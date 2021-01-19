//! # End Event flow node
use crate::bpmn::schema::{EndEvent as Element, FlowNodeType};
use crate::event::ProcessEvent;
use crate::flow_node::{self, Action, FlowNode, IncomingIndex};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use tokio::sync::broadcast;

/// End Event flow node
pub struct EndEvent {
    element: Arc<Element>,
    state: State,
    waker: Option<Waker>,
    event_broadcaster: Option<broadcast::Sender<ProcessEvent>>,
}

impl EndEvent {
    /// Creates new End Event flow node
    pub fn new(element: Element) -> Self {
        Self {
            element: Arc::new(element),
            state: State::Ready,
            waker: None,
            event_broadcaster: None,
        }
    }
}

/// Node state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum State {
    Ready,
    Complete,
    Done,
}

impl FlowNode for EndEvent {
    fn set_state(&mut self, state: flow_node::State) -> Result<(), flow_node::StateError> {
        match state {
            flow_node::State::EndEvent(state) => {
                self.state = state;
                Ok(())
            }
            _ => Err(flow_node::StateError::InvalidVariant),
        }
    }

    fn get_state(&self) -> flow_node::State {
        flow_node::State::EndEvent(self.state.clone())
    }

    fn element(&self) -> Box<dyn FlowNodeType> {
        Box::new(self.element.as_ref().clone())
    }

    fn incoming(&mut self, _index: IncomingIndex) {
        // Any incoming triggered is good enough to
        // complete EndEvent
        self.state = State::Complete;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    fn set_process(&mut self, process: crate::process::Handle) {
        self.event_broadcaster.replace(process.event_broadcast());
    }
}

impl From<Element> for EndEvent {
    fn from(element: Element) -> Self {
        Self::new(element)
    }
}

impl Stream for EndEvent {
    type Item = Action;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.state {
            State::Ready => {
                self.waker.replace(cx.waker().clone());
                Poll::Pending
            }
            State::Complete => {
                self.state = State::Done;
                if let Some(event_broadcaster) = self.event_broadcaster.as_ref() {
                    let _ = event_broadcaster.send(ProcessEvent::End);
                }
                Poll::Ready(Some(Action::Complete))
            }
            // There's no way to wake up EndEvent from this state
            // as end means end, period.
            State::Done => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bpmn::schema::*;
    use crate::event::ProcessEvent;
    use crate::model;
    use crate::test::Mailbox;

    #[tokio::test]
    async fn start_throws_event() {
        let mut proc1: Process = Default::default();
        proc1.id = Some("proc1".into());
        let mut start_event: StartEvent = Default::default();
        start_event.id = Some("start".into());
        let mut end_event: EndEvent = Default::default();
        end_event.id = Some("end".into());
        let seq_flow = establish_sequence_flow(&mut start_event, &mut end_event, "s1").unwrap();
        proc1
            .flow_elements
            .push(FlowElement::StartEvent(start_event));
        proc1.flow_elements.push(FlowElement::EndEvent(end_event));
        proc1
            .flow_elements
            .push(FlowElement::SequenceFlow(seq_flow));

        let mut definitions: Definitions = Default::default();
        definitions
            .root_elements
            .push(RootElement::Process(proc1.clone()));
        let model = model::Model::new(definitions).spawn().await;

        let handle = model.processes().await.unwrap().pop().unwrap();
        let mut mailbox = Mailbox::new(handle.event_receiver());

        assert!(handle.start().await.is_ok());

        assert!(mailbox.receive(|e| matches!(e, ProcessEvent::End)).await);
    }
}