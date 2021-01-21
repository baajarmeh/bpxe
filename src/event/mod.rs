//! # Event subsystem
pub mod start_event;
pub use start_event::StartEvent;
pub mod end_event;
pub use end_event::EndEvent;
pub mod intermediate_throw_event;
pub use intermediate_throw_event::IntermediateThrowEvent;

use crate::bpmn::schema::*;
use std::convert::TryFrom;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ProcessEvent {
    /// Process has started
    Start,
    /// Process has ended
    End,
    /// None Event was thrown
    NoneEvent,
    /// Signal Event
    SignalEvent { signal_ref: Option<String> },
    /// Cancel Event
    CancelEvent,
    /// Terminate Event
    TerminateEvent,
    /// Compensation Event
    CompensationEvent { activity_ref: Option<String> },
    /// Message Event
    MessageEvent {
        message_ref: Option<String>,
        operation_ref: Option<OperationRef>,
    },
    /// Escalation Event
    EscalationEvent { escalation_ref: Option<String> },
    /// Link Event
    LinkEvent {
        sources: Vec<Source>,
        target: Option<Target>,
    },
    /// Error Event
    ErrorEvent { error_ref: Option<String> },
}

/// Event conversion error
pub enum ConversionError {
    /// Event can't be converted
    Impossible,
    /// Event can be converted, but this hasn't been implemented yet
    NotImplemented,
}

impl TryFrom<CancelEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(_event_definition: CancelEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::CancelEvent)
    }
}

impl TryFrom<TerminateEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(_event_definition: TerminateEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::TerminateEvent)
    }
}

impl TryFrom<CompensateEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(event_definition: CompensateEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::CompensationEvent {
            activity_ref: event_definition.activity_ref,
        })
    }
}

impl TryFrom<SignalEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(event_definition: SignalEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::SignalEvent {
            signal_ref: event_definition.signal_ref,
        })
    }
}

impl TryFrom<MessageEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(event_definition: MessageEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::MessageEvent {
            message_ref: event_definition.message_ref,
            operation_ref: event_definition.operation_ref,
        })
    }
}

impl TryFrom<EscalationEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(event_definition: EscalationEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::EscalationEvent {
            escalation_ref: event_definition.escalation_ref,
        })
    }
}

impl TryFrom<LinkEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(event_definition: LinkEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::LinkEvent {
            sources: event_definition.sources,
            target: event_definition.target,
        })
    }
}

impl TryFrom<ErrorEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(event_definition: ErrorEventDefinition) -> Result<Self, Self::Error> {
        Ok(ProcessEvent::ErrorEvent {
            error_ref: event_definition.error_ref,
        })
    }
}

impl TryFrom<ConditionalEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(_event_definition: ConditionalEventDefinition) -> Result<Self, Self::Error> {
        Err(ConversionError::Impossible)
    }
}

impl TryFrom<TimerEventDefinition> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(_event_definition: TimerEventDefinition) -> Result<Self, Self::Error> {
        Err(ConversionError::Impossible)
    }
}

impl<T> From<Box<T>> for ConversionError {
    fn from(_: Box<T>) -> Self {
        Self::NotImplemented
    }
}

impl TryFrom<Box<dyn EventDefinitionType>> for ProcessEvent {
    type Error = ConversionError;
    fn try_from(mut event_definition: Box<dyn EventDefinitionType>) -> Result<Self, Self::Error> {
        match event_definition.downcast::<CancelEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        match event_definition.downcast::<TerminateEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        match event_definition.downcast::<CompensateEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        match event_definition.downcast::<MessageEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }

        match event_definition.downcast::<EscalationEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }

        match event_definition.downcast::<LinkEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        match event_definition.downcast::<ErrorEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        match event_definition.downcast::<ConditionalEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        match event_definition.downcast::<TimerEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(e) => event_definition = e,
        }
        #[allow(clippy::single_match)] // want to keep using the same pattern
        match event_definition.downcast::<SignalEventDefinition>() {
            Ok(e) => return ProcessEvent::try_from(*e),
            Err(_) => {}
        }

        Err(ConversionError::NotImplemented)
    }
}
