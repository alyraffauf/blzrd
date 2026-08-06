use std::collections::HashMap;
use std::fmt::Display;
use std::io::{self, IsTerminal};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use owo_colors::{OwoColorize, Stream};

pub struct Ui {
    progress: MultiProgress,
    uses_live_progress: bool,
    needs_section_separator: Mutex<bool>,
}

pub struct SectionProgress {
    name: String,
    progress: ProgressBar,
    jobs: HashMap<String, JobProgress>,
    job_order: Vec<String>,
}

#[derive(Clone)]
pub struct JobProgress {
    progress: ProgressBar,
    result: Arc<Mutex<Option<String>>>,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            progress: MultiProgress::new(),
            uses_live_progress: io::stderr().is_terminal(),
            needs_section_separator: Mutex::new(false),
        }
    }

    pub fn print_warning(message: impl Display) {
        eprintln!("{}", warning(format!("! {message}")));
    }

    pub fn start_section(&self, name: &str) -> SectionProgress {
        self.add_section_separator();

        let heading = phase_heading(name);
        if !self.uses_live_progress {
            eprintln!("⠋ {heading}");
        }

        SectionProgress {
            name: heading.clone(),
            progress: self.new_section_spinner(heading),
            jobs: HashMap::new(),
            job_order: Vec::new(),
        }
    }

    pub fn add_job(&self, section: &mut SectionProgress, name: &str, display_name: &str) {
        if section.jobs.contains_key(name) {
            return;
        }

        let progress = JobProgress {
            progress: self.new_job_spinner(display_name),
            result: Arc::new(Mutex::new(None)),
        };
        section.jobs.insert(name.to_owned(), progress);
        section.job_order.push(name.to_owned());
    }

    pub fn start_job(
        &self,
        section: &SectionProgress,
        name: &str,
        display_name: &str,
    ) -> JobProgress {
        let progress = section
            .jobs
            .get(name)
            .expect("job progress must be registered")
            .clone();
        progress
            .progress
            .enable_steady_tick(Duration::from_millis(100));
        progress.progress.tick();

        if !self.uses_live_progress {
            eprintln!("  ⠋ {display_name}");
        }

        progress
    }

    pub fn finish_job_success(&self, progress: &JobProgress, name: &str) {
        self.finish_job(progress, format!("  {} {name}", success_marker()));
    }

    pub fn finish_job_failure(&self, progress: &JobProgress, name: &str, error: impl Display) {
        self.finish_job(progress, format!("  {} {name}: {error}", failure_marker()));
    }

    pub fn job_progress<'a>(section: &'a SectionProgress, name: &str) -> &'a JobProgress {
        section
            .jobs
            .get(name)
            .expect("job progress must be registered")
    }

    pub fn finish_section_success(&self, section: &SectionProgress) {
        self.finish_section(section, true);
    }

    pub fn finish_section_failure(&self, section: &SectionProgress) {
        self.finish_section(section, false);
    }

    pub fn print_summary(&self, message: &str) {
        self.add_section_separator();
        let message = format!("{} {}", success_marker(), phase_heading(message));
        eprintln!("{message}");
    }

    fn new_section_spinner(&self, message: impl Into<String>) -> ProgressBar {
        self.new_spinner(message, spinner_style(), true)
    }

    fn new_job_spinner(&self, message: impl Into<String>) -> ProgressBar {
        self.new_spinner(message, job_spinner_style(), false)
    }

    fn new_spinner(
        &self,
        message: impl Into<String>,
        style: ProgressStyle,
        should_animate: bool,
    ) -> ProgressBar {
        let progress = self.progress.add(ProgressBar::new_spinner());
        progress.set_style(style);
        progress.set_message(message.into());
        progress.tick();
        if should_animate {
            progress.enable_steady_tick(Duration::from_millis(100));
        }
        progress
    }

    fn finish_job(&self, progress: &JobProgress, message: String) {
        *progress
            .result
            .lock()
            .expect("job result mutex is not poisoned") = Some(message.clone());

        if self.uses_live_progress {
            progress.progress.set_style(completed_style());
            progress.progress.finish_with_message(message);
        } else {
            eprintln!("{message}");
            progress.progress.finish_and_clear();
        }
    }

    fn finish_section(&self, section: &SectionProgress, is_success: bool) {
        let message = if is_success {
            format!("{} {}", success_marker(), section.name)
        } else {
            format!("{} {}", failure_marker(), section.name)
        };

        if !is_success {
            for progress in section.jobs.values() {
                if !progress.progress.is_finished() {
                    progress.progress.finish_and_clear();
                }
            }
        }

        if self.uses_live_progress {
            section.progress.finish_and_clear();
            self.progress
                .clear()
                .expect("clearing completed progress must succeed");
            eprintln!("{message}");
            for name in &section.job_order {
                let progress = &section.jobs[name];
                if let Some(result) = progress
                    .result
                    .lock()
                    .expect("job result mutex is not poisoned")
                    .as_deref()
                {
                    eprintln!("{result}");
                }
            }
        } else {
            eprintln!("{message}");
            section.progress.finish_and_clear();
        }

        self.progress.remove(&section.progress);
        for progress in section.jobs.values() {
            self.progress.remove(&progress.progress);
        }
        *self
            .needs_section_separator
            .lock()
            .expect("section separator mutex is not poisoned") = true;
    }

    fn add_section_separator(&self) {
        let mut needs_separator = self
            .needs_section_separator
            .lock()
            .expect("section separator mutex is not poisoned");
        if !*needs_separator {
            return;
        }
        *needs_separator = false;
        drop(needs_separator);

        eprintln!();
    }
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner} {msg}").expect("spinner template is valid")
}

fn job_spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("  {spinner} {msg}").expect("spinner template is valid")
}

fn completed_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg}").expect("completed template is valid")
}

fn success_marker() -> String {
    format!(
        "{}",
        "✓".if_supports_color(Stream::Stderr, |marker| marker.green())
    )
}

fn failure_marker() -> String {
    format!(
        "{}",
        "✗".if_supports_color(Stream::Stderr, |marker| marker.red())
    )
}

fn phase_heading(name: &str) -> String {
    let uppercase_name = name.to_uppercase();
    format!(
        "{}",
        uppercase_name.if_supports_color(Stream::Stderr, |heading| heading.bold())
    )
}

fn warning(message: impl Display) -> String {
    format!(
        "{}",
        message.if_supports_color(Stream::Stderr, |message| message.yellow())
    )
}
