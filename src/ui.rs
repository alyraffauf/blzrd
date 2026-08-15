use std::fmt::Display;
use std::sync::Mutex;

pub struct Ui {
    active_phase: Mutex<Option<String>>,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            active_phase: Mutex::new(None),
        }
    }

    pub fn print_warning(message: impl Display) {
        eprintln!("! {message}");
    }

    pub fn start_section(&self, phase: &str) {
        let mut active_phase = self
            .active_phase
            .lock()
            .expect("active phase mutex is not poisoned");
        assert!(active_phase.is_none(), "a phase is already active");

        eprintln!("{}", phase.to_uppercase());
        *active_phase = Some(phase.to_owned());
    }

    pub fn finish_job_success(&self, job: &str) {
        self.assert_phase_is_active();
        eprintln!("  ✓ {job}");
    }

    pub fn finish_job_failure(&self, job: &str, error: impl Display) {
        self.assert_phase_is_active();
        eprintln!("  ✗ {job}: {error}");
    }

    pub fn finish_section_success(&self) {
        self.take_active_phase();
        eprintln!();
    }

    pub fn finish_section_failure(&self) {
        let phase = self.take_active_phase();
        eprintln!("✗ {} FAILED\n", phase.to_uppercase());
    }

    pub fn finalize_success(&self) {
        self.assert_no_phase_is_active();
        eprintln!("✓ DEPLOYMENT COMPLETE");
    }

    pub fn finalize_failure(&self) {
        self.assert_no_phase_is_active();
        eprintln!("✗ DEPLOYMENT FAILED");
    }

    fn assert_phase_is_active(&self) {
        assert!(
            self.active_phase
                .lock()
                .expect("active phase mutex is not poisoned")
                .is_some(),
            "a job can only change in an active phase"
        );
    }

    fn assert_no_phase_is_active(&self) {
        assert!(
            self.active_phase
                .lock()
                .expect("active phase mutex is not poisoned")
                .is_none(),
            "a run cannot finish with an active phase"
        );
    }

    fn take_active_phase(&self) -> String {
        self.active_phase
            .lock()
            .expect("active phase mutex is not poisoned")
            .take()
            .expect("a phase must be active when it finishes")
    }
}
