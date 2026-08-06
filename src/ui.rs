use std::collections::HashMap;
use std::fmt::Display;
use std::io::{self, IsTerminal};
use std::sync::Mutex;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use owo_colors::{OwoColorize, Stream};

pub struct Ui {
    progress: MultiProgress,
    is_interactive: bool,
    retained_progress: Mutex<Vec<ProgressBar>>,
    needs_section_separator: Mutex<bool>,
}

pub struct SectionProgress {
    name: String,
    progress: ProgressBar,
    jobs: HashMap<String, ProgressBar>,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            progress: MultiProgress::new(),
            is_interactive: io::stderr().is_terminal(),
            retained_progress: Mutex::new(Vec::new()),
            needs_section_separator: Mutex::new(false),
        }
    }

    pub fn print_warning(message: impl Display) {
        eprintln!("{}", warning(format!("! {message}")));
    }

    pub fn start_section(&self, name: &str) -> SectionProgress {
        self.add_section_separator();

        let heading = phase_heading(name);
        if !self.is_interactive {
            eprintln!("⠋ {heading}");
        }

        SectionProgress {
            name: heading.clone(),
            progress: self.new_spinner(heading, true),
            jobs: HashMap::new(),
        }
    }

    pub fn add_job(&self, section: &mut SectionProgress, name: &str, display_name: &str) {
        if section.jobs.contains_key(name) {
            return;
        }

        let progress = self.new_spinner(display_name, false);
        progress.set_style(job_spinner_style());
        section.jobs.insert(name.to_owned(), progress);
    }

    pub fn start_job(
        &self,
        section: &SectionProgress,
        name: &str,
        display_name: &str,
    ) -> ProgressBar {
        let progress = section
            .jobs
            .get(name)
            .expect("job progress must be registered")
            .clone();
        progress.enable_steady_tick(Duration::from_millis(100));
        progress.tick();

        if !self.is_interactive {
            eprintln!("  ⠋ {display_name}");
        }

        progress
    }

    pub fn finish_job_success(&self, progress: &ProgressBar, name: &str) {
        self.finish_job(progress, format!("  {} {name}", success_marker()));
    }

    pub fn finish_job_failure(&self, progress: &ProgressBar, name: &str, error: impl Display) {
        self.finish_job(progress, format!("  {} {name}: {error}", failure_marker()));
    }

    pub fn job_progress<'a>(section: &'a SectionProgress, name: &str) -> &'a ProgressBar {
        section
            .jobs
            .get(name)
            .expect("job progress must be registered")
    }

    pub fn finish_section_success(&self, section: SectionProgress) {
        self.finish_section(section, true);
    }

    pub fn finish_section_failure(&self, section: SectionProgress) {
        self.finish_section(section, false);
    }

    pub fn print_summary(&self, message: &str) {
        self.add_section_separator();
        let message = format!("{} {}", success_marker(), phase_heading(message));
        if self.is_interactive {
            let progress = self.progress.add(ProgressBar::new_spinner());
            progress.set_style(completed_style());
            progress.finish_with_message(message);
            self.retain_progress(progress);
        } else {
            eprintln!("{message}");
        }
    }

    fn new_spinner(&self, message: impl Into<String>, should_animate: bool) -> ProgressBar {
        let progress = self.progress.add(ProgressBar::new_spinner());
        progress.set_style(spinner_style());
        progress.set_message(message.into());
        progress.tick();
        if should_animate {
            progress.enable_steady_tick(Duration::from_millis(100));
        }
        progress
    }

    fn finish_job(&self, progress: &ProgressBar, message: String) {
        if self.is_interactive {
            progress.set_style(completed_style());
            progress.finish_with_message(message);
        } else {
            eprintln!("{message}");
            progress.finish_and_clear();
        }
    }

    fn finish_section(&self, section: SectionProgress, is_success: bool) {
        let message = if is_success {
            format!("{} {}", success_marker(), section.name)
        } else {
            format!("{} {}", failure_marker(), section.name)
        };

        if !is_success {
            for progress in section.jobs.values() {
                if !progress.is_finished() {
                    progress.finish_and_clear();
                }
            }
        }

        if self.is_interactive {
            section.progress.set_style(completed_style());
            section.progress.finish_with_message(message);
        } else {
            eprintln!("{message}");
            section.progress.finish_and_clear();
        }

        self.retain(section);
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

        if self.is_interactive {
            let separator = self.progress.add(ProgressBar::new_spinner());
            separator.set_style(completed_style());
            separator.finish_with_message(" ".to_owned());
            self.retain_progress(separator);
        } else {
            eprintln!();
        }
    }

    fn retain(&self, section: SectionProgress) {
        let mut retained = self
            .retained_progress
            .lock()
            .expect("progress retention mutex is not poisoned");
        retained.push(section.progress);
        retained.extend(section.jobs.into_values());
    }

    fn retain_progress(&self, progress: ProgressBar) {
        self.retained_progress
            .lock()
            .expect("progress retention mutex is not poisoned")
            .push(progress);
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
