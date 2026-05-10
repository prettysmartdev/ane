use std::process::Child;

pub enum HealthStatus {
    Healthy,
    ProcessExited(Option<i32>),
}

pub fn check_process(child: &mut Child) -> HealthStatus {
    match child.try_wait() {
        Ok(Some(status)) => HealthStatus::ProcessExited(status.code()),
        Ok(None) => HealthStatus::Healthy,
        Err(_) => HealthStatus::ProcessExited(None),
    }
}
