//! Process scheduler
//!
//! This is where the magic happens
use crate::bpmn::schema::{
    DocumentElementContainer, Element as E, Expr, FormalExpression, ProcessType, SequenceFlow,
};
use crate::event::ProcessEvent as Event;
use crate::flow_node;
use crate::language::ExpressionEvaluator;

use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt, StreamFuture};
use std::future::Future;
use std::pin::Pin;

use std::task::{Context, Poll};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::{self};

use super::{Handle, Log, Request, StartError};

pub(crate) struct Scheduler {
    receiver: mpsc::Receiver<Request>,
    process: Handle,
    flow_nodes: FuturesUnordered<FlowNode>,
}

// FIXME: We're using this structure to be able to find flow nodes by their identifier
// in `FuturesUnordered` (`Scheduler.flow_nodes`). It's a linear search and is probably
// fine when there's a small number of flow nodes, but should it become large, this approach
// should probably be rethought.
struct FlowNode {
    id: String,
    future: StreamFuture<Box<dyn flow_node::FlowNode>>,
    tokens: usize,
}

use std::ops::{Deref, DerefMut};

impl Deref for FlowNode {
    type Target = Box<dyn flow_node::FlowNode>;

    fn deref(&self) -> &Self::Target {
        // FIXME: is there any better way to do this?
        // I *think* it's reasonable to assume it won't panic in runtime
        // because when it's used, scheduler is not doing anything with the future.
        // However, I am not confident in this.
        self.future.get_ref().unwrap()
    }
}

impl DerefMut for FlowNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // FIXME: see above in `Deref` implementation
        self.future.get_mut().unwrap()
    }
}

/// This encapsulates an item produced by flow node (as a Stream)
struct Next {
    id: String,
    item: <StreamFuture<Box<dyn flow_node::FlowNode>> as Future>::Output,
    tokens: usize,
}

impl Future for FlowNode {
    type Output = Next;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.future.poll_unpin(cx).map(|v| Next {
            id: self.id.clone(),
            item: v,
            tokens: self.tokens,
        })
    }
}

impl Scheduler {
    pub(crate) fn new(receiver: mpsc::Receiver<Request>, process: Handle) -> Self {
        let flow_nodes = process
            .element()
            .flow_elements()
            .iter()
            .map(|e| e.clone().into_inner())
            .filter_map(|e| {
                flow_node::new(e.as_ref()).map(|mut flow_node| {
                    flow_node.set_process(process.clone());
                    let e = flow_node.element();
                    FlowNode {
                        // FIXME: decide what should we do with flow nodes that don't have ID.
                        // They can't be connected with other nodes (there's no way to refer to
                        // them), but they can still be operational in a single flow node operation
                        // (even though this might be a degenerative case)
                        id: e.id().as_ref().unwrap_or(&"".to_string()).to_string(),
                        future: flow_node.into_future(),
                        tokens: 0,
                    }
                })
            })
            .collect();
        Self {
            receiver,
            process,
            flow_nodes,
        }
    }

    // Main loop
    pub async fn run(mut self) {
        let mut join_handle = None;
        let element = self.process.element();
        let log_broadcast = self.process.log_broadcast();
        let expression_evaluator: ExpressionEvaluator = Default::default();
        let default_expression_language = self
            .process
            .model()
            .definitions()
            .expression_language
            .clone();

        // This function is async even though nothing in it is asynchronous
        // at this moment. This is done with an expectation that expression
        // evaluation *might* become asynchronous in the future.
        async fn probe_sequence_flow(
            expression_evaluator: &ExpressionEvaluator,
            seq_flow: &SequenceFlow,
            default_expression_language: Option<&String>,
            log_broadcast: broadcast::Sender<Log>,
        ) -> bool {
            if let Some(Expr::FormalExpression(FormalExpression {
                content: Some(ref content),
                ..
            })) = seq_flow.condition_expression
            {
                match expression_evaluator.eval_expr(default_expression_language, content) {
                    Ok(result) => result,
                    Err(err) => {
                        let _ = log_broadcast.send(Log::ExpressionError {
                            error: format!("{:?}", err),
                        });
                        false
                    }
                }
            } else {
                true
            }
        }
        loop {
            task::yield_now().await;
            tokio::select! {
               next = self.receiver.recv()  =>
                   match next {
                       Some(Request::JoinHandle(handle)) => join_handle = Some(handle),
                       Some(Request::Terminate(sender)) => {
                           let _ = sender.send(join_handle.take());
                           return;
                       }
                       Some(Request::Start(sender)) => {
                           self.start(sender);
                       }
                       None => {}
                   },
               next = self.flow_nodes.next() => {
                   if let Some(Next{id, item: (action, mut flow_node), tokens}) = next  {
                       // Figure out if this action should be transformed, kept as is, or dropped
                       enum Control {
                           Proceed(Option<flow_node::Action>),
                           Drop
                       }
                       let control = flow_node.element().incomings().iter().
                           fold(Control::Proceed(action), |control, incoming| {
                               match control {
                                   // once the action has been dropped, it's not checked against
                                   // any other incoming flows
                                   Control::Drop => control,
                                   Control::Proceed(action) => {
                                       let mut matching_predecessor = self.flow_nodes.iter_mut().find(|node|
                                           node.element().outgoings().iter()
                                           .any(|outgoing| outgoing == incoming));
                                           if let Some(ref mut node) = matching_predecessor {
                                               // it's ok to unwrap here because we already know such
                                               // predecessor exists
                                               let index = node.element().outgoings().iter().
                                                   enumerate().find_map(|(i, name)| if name == incoming {
                                                       Some(i)
                                                   } else {
                                                       None
                                                   }).unwrap();
                                               match node.handle_outgoing_action(index, action) {
                                                   None => Control::Drop,
                                                   Some(action) => Control::Proceed(action),
                                                   }
                                           } else {
                                               Control::Proceed(action)
                                           }
                                   }
                               }
                           });
                       match control {
                           Control::Proceed(Some(flow_node::Action::ProbeOutgoingSequenceFlows(indices))) => {
                               let outgoings = flow_node.element().outgoings().clone();
                               for index in indices {
                                   let seq_flow = {
                                       element.find_by_id(&outgoings[index])
                                           .and_then(|seq_flow| seq_flow.downcast_ref::<SequenceFlow>())
                                   };
                                   if let Some(seq_flow) = seq_flow {
                                       let success = probe_sequence_flow(&expression_evaluator, &seq_flow,
                                           default_expression_language.as_ref(),
                                           log_broadcast.clone()).await;
                                       flow_node.sequence_flow(index, &seq_flow, success);
                                   }
                               }
                           }
                           Control::Proceed(Some(flow_node::Action::Flow(ref indices))) => {
                               let el = flow_node.element();
                               let outgoings = el.outgoings();
                               for index in indices {
                                   // FIXME: see above about ID-less flow nodes
                                   let seq_flow = {
                                       element.find_by_id(&outgoings[*index])
                                           .and_then(|seq_flow| seq_flow.downcast_ref::<SequenceFlow>())
                                   };

                                   if let Some(seq_flow) = seq_flow {
                                       let success = probe_sequence_flow(&expression_evaluator, &seq_flow,
                                           default_expression_language.as_ref(),
                                           log_broadcast.clone()).await;
                                       if success {
                                           for next_node in self.flow_nodes.iter_mut() {
                                               if next_node.id == seq_flow.target_ref {
                                                   let target_node = &mut next_node.future;
                                                   if let Some(node) = target_node.get_mut() {
                                                       let index = node.element().incomings().iter().enumerate().
                                                           find_map(|(index, incoming)|
                                                               if incoming == seq_flow.id.as_ref().unwrap() {
                                                                   Some(index)
                                                               } else {
                                                                   None
                                                               });

                                                       if let Some(index) = index {
                                                           let _ = log_broadcast.send(Log::FlowNodeIncoming {
                                                               node: node.element().clone(),
                                                               incoming_index: index
                                                           });
                                                           // increase the number of tokens by a number of added flows
                                                           next_node.tokens += indices.len();
                                                           node.tokens(next_node.tokens);
                                                           node.incoming(index);
                                                       }
                                                   }
                                               }
                                           }
                                       }
                                   }
                               }
                           }
                           Control::Proceed(Some(flow_node::Action::Complete)) => {
                               let _ = log_broadcast.send(Log::FlowNodeCompleted { node: flow_node.element().clone() });
                           }
                           Control::Proceed(None) => {
                               if self.flow_nodes.is_empty() {
                                   let _ = log_broadcast.send(Log::Done);
                               }
                               continue
                           }
                           Control::Drop => {}
                       }
                       // Reschedule the flow node
                       self.flow_nodes.push(FlowNode{id, future: flow_node.into_future(), tokens});
                   }
               },
            }
        }
    }

    fn start(&mut self, sender: oneshot::Sender<Result<(), StartError>>) {
        if !self
            .process
            .element()
            .flow_elements()
            .iter()
            .map(|e| e.clone().into_inner())
            .any(|node| node.element() == E::StartEvent)
        {
            let _ = sender.send(Err(StartError::NoStartEvent));
        } else {
            let event_broadcast = self.process.event_broadcast();
            let _ = event_broadcast.send(Event::Start);
            let _ = sender.send(Ok(()));
        }
    }
}
