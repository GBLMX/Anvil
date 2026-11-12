//! Task command execution and problem-panel integration.

use super::*;
use crate::task_runner::{TaskEvent, TaskWorker};
use anvil_session::ui::Problem;

impl Model {
    pub(crate) fn start_task(&mut self, command: &str) {
        self.start_task_mode(command, false);
    }

    pub(crate) fn start_tests(&mut self, command: &str) {
        self.start_task_mode(command, true);
    }

    fn start_task_mode(&mut self, command: &str, tests: bool) {
        let command = command.trim();
        if command.is_empty() {
            self.status = if tests {
                "E: usage: :tests <command>".to_owned()
            } else {
                "E: usage: :task <command>".to_owned()
            };
            self.dirty = true;
            return;
        }
        if self.reject_read_only() {
            return;
        }
        if self.task_worker.is_some() {
            self.status = "E: task already running".to_owned();
            self.dirty = true;
            return;
        }
        self.task_problems.clear();
        self.task_output = None;
        self.task_is_tests = tests;
        self.overlays.test_explorer = None;
        self.rebuild_problems();
        self.task_worker = Some(TaskWorker::start(
            command.to_owned(),
            self.workspace.clone(),
        ));
        self.status = if tests {
            format!("tests: running {command}")
        } else {
            format!("task: running {command}")
        };
        self.dirty = true;
    }

    pub(crate) fn poll_task(&mut self) {
        let Some(event) = self.task_worker.as_ref().and_then(TaskWorker::try_recv) else {
            return;
        };
        self.task_worker = None;
        let TaskEvent::Finished(result) = event;
        let tests = self.task_is_tests;
        self.task_is_tests = false;
        self.task_output = Some(result.output.clone());
        self.task_problems = result
            .problems
            .into_iter()
            .map(|problem| Problem {
                path: problem.path,
                line: problem.line,
                character: problem.character,
                severity: None,
                source: Some(if tests { "test" } else { "task" }.to_owned()),
                code: None,
                message: problem.message,
            })
            .collect();
        self.rebuild_problems();
        if tests {
            self.overlays.test_explorer = Some(crate::test_explorer::TestExplorerState::new(
                self.task_problems.clone(),
            ));
            self.ui.problems.visible = false;
        } else if !self.task_problems.is_empty() {
            self.ui.problems.visible = true;
            self.ui.problems.selected = 0;
        }
        let label = if tests { "tests" } else { "task" };
        let mut status = if result.timed_out {
            format!("E: {label} timed out")
        } else {
            match result.exit_code {
                Some(0) => format!("{label}: ok"),
                Some(code) => format!("E: {label} exited {code}"),
                None => format!("E: {label} ended without status"),
            }
        };
        if !self.task_problems.is_empty() {
            status.push_str(&format!("; {} problems", self.task_problems.len()));
        }
        if result.truncated {
            status.push_str("; output truncated");
        }
        self.status = status;
        self.dirty = true;
    }

    pub(crate) fn show_task_output(&mut self) {
        let Some(output) = self.task_output.clone() else {
            self.status = "no task output".to_owned();
            self.dirty = true;
            return;
        };
        self.show_read_only(&output, "task output: :q returns", "task output");
    }
}
