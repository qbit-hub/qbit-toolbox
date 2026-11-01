//! Platform-neutral, allocation-free runtime primitives.
pub mod metrics;
pub mod modifiers;
pub mod planner;

pub use metrics::{Metrics, MetricsSnapshot};
pub use modifiers::{ConcreteModifier, ModifierState};
pub use planner::{KeystrokePlan, PlanStep, PlannerError, plan_keystroke};
