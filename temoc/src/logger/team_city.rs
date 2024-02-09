use aho_corasick::AhoCorasick;
use anyhow::{bail, Result};
use chrono::Utc;
use std::io::Write;
use ulid::Ulid;

use crate::processor::Position;

use super::TestLogger;

pub struct TeamCityTestLogger<Output> {
    flow_id: Ulid,
    state: TeamCityTestLoggerState,
    output: Output,
}

impl<W> TeamCityTestLogger<W>
where
    W: Write,
{
    pub fn new(mut output: W) -> Self {
        writeln!(output, "\n##teamcity[enteredTheMatrix]").expect("Error creatting logger");
        Self {
            flow_id: Ulid::new(),
            state: TeamCityTestLoggerState::Created,
            output,
        }
    }

    fn write_message(&mut self, event: TeamCityEvent) -> Result<()> {
        match event {
            TeamCityEvent::TestCount { count } => {
                writeln!(
                    self.output,
                    "##teamcity[testCount count='{count}' flowId='{}']",
                    self.flow_id
                )?;
            }
            TeamCityEvent::TestSuiteStarted { name } => {
                writeln!(
                    self.output,
                    "##teamcity[testSuiteStarted name='{name}' file='{name}' flowId='{}']",
                    self.flow_id
                )?;
            }
            TeamCityEvent::TestSuiteFinished { name } => {
                writeln!(
                    self.output,
                    "##teamcity[testSuiteFinished name='{name}' flowId='{}']",
                    self.flow_id
                )?;
            }
            TeamCityEvent::TestStarted { name, position } => {
                writeln!(
                    self.output,
                    "##teamcity[testStarted name='{name}' line='{}' captureStandardOutput='true' flowId='{}']",
                    position.line, self.flow_id
                )?;
            }
            TeamCityEvent::TestFinished { name, duration } => {
                writeln!(
                    self.output,
                    "##teamcity[testFinished name='{name}' duration='{duration}' flowId='{}']",
                    self.flow_id
                )?;
            }
            TeamCityEvent::TestIgnored {
                name,
                message,
                duration,
            } => {
                writeln!(
                    self.output,
                    "##teamcity[testIgnored name='{name}' message='{message}' duration='{duration}' flowId='{}']",
                     self.flow_id
                )?;
            }
            TeamCityEvent::TestFailed {
                name,
                message,
                duration,
            } => {
                writeln!(
                    self.output,
                    "##teamcity[testFailed name='{name}' message='{message}' duration='{duration}' flowId='{}']",
                     self.flow_id
                )?;
            }
        }
        Ok(())
    }

    fn escape(text: &str) -> String {
        let patterns = &["|", "'", "\n", "\r", "]", "["];
        let replace_with = &["||", "|'", "|n", "|r", "|]", "|["];
        let ac = AhoCorasick::new(patterns).unwrap();
        ac.replace_all(text, replace_with)
    }
}

impl<W> TestLogger for TeamCityTestLogger<W>
where
    W: Write,
{
    fn number_of_tests(&mut self, n: usize) -> anyhow::Result<()> {
        self.write_message(TeamCityEvent::TestCount { count: n as u64 })
    }

    fn file_started(&mut self, name: &str) -> anyhow::Result<()> {
        self.write_message(TeamCityEvent::TestSuiteStarted { name: &Self::escape(name) })?;
        self.state = TeamCityTestLoggerState::StartedTestSuite { name: name.into() };
        Ok(())
    }

    fn file_finished(&mut self) -> anyhow::Result<()> {
        let TeamCityTestLoggerState::StartedTestSuite { name } = self.state.clone() else {
            bail!("Unexpected state in logger found!");
        };
        self.write_message(TeamCityEvent::TestSuiteFinished { name: &Self::escape(&name) })?;
        self.state = TeamCityTestLoggerState::Created;
        Ok(())
    }

    fn row_started(
        &mut self,
        decision_table: &str,
        row_number: usize,
        position: Position,
    ) -> Result<()> {
        let TeamCityTestLoggerState::StartedTestSuite { name } = &self.state.clone() else {
            bail!("Unexpected state in logger found!");
        };
        let test_name = format!("{decision_table}({row_number})");
        self.write_message(TeamCityEvent::TestStarted {
            name: &Self::escape(&test_name),
            position,
        })?;
        self.state = TeamCityTestLoggerState::StartedTest {
            test_suite_name: name.to_string(),
            test_name,
            started_at: Utc::now().timestamp_millis(),
        };
        Ok(())
    }

    fn row_failed(&mut self, message: &str) -> anyhow::Result<()> {
        let TeamCityTestLoggerState::StartedTest {
            test_suite_name: _,
            test_name,
            started_at,
        } = &self.state.clone()
        else {
            bail!("Unexpected state in logger found!");
        };
        self.write_message(TeamCityEvent::TestFailed {
            name: &Self::escape(&test_name),
            message: &Self::escape(message),
            duration: Utc::now().timestamp_millis() - started_at,
        })?;
        Ok(())
    }

    fn row_snoozed(&mut self, message: &str) -> anyhow::Result<()> {
        let TeamCityTestLoggerState::StartedTest {
            test_suite_name: _,
            test_name,
            started_at,
        } = &self.state.clone()
        else {
            bail!("Unexpected state in logger found!");
        };
        self.write_message(TeamCityEvent::TestIgnored {
            name: &Self::escape(&test_name),
            message: &Self::escape(message),
            duration: Utc::now().timestamp_millis() - started_at,
        })?;
        Ok(())
    }

    fn row_finished(&mut self) -> anyhow::Result<()> {
        let TeamCityTestLoggerState::StartedTest {
            test_suite_name,
            test_name,
            started_at,
        } = &self.state.clone()
        else {
            bail!("Unexpected state in logger found!");
        };
        self.write_message(TeamCityEvent::TestFinished {
            name: &Self::escape(&test_name),
            duration: Utc::now().timestamp_millis() - started_at,
        })?;
        self.state = TeamCityTestLoggerState::StartedTestSuite {
            name: test_suite_name.into(),
        };
        Ok(())
    }
}

#[derive(Clone)]
enum TeamCityTestLoggerState {
    Created,
    StartedTestSuite {
        name: String,
    },
    StartedTest {
        test_suite_name: String,
        test_name: String,
        started_at: i64,
    },
}

enum TeamCityEvent<'a> {
    TestCount {
        count: u64,
    },
    TestSuiteStarted {
        name: &'a str,
    },
    TestSuiteFinished {
        name: &'a str,
    },
    TestStarted {
        name: &'a str,
        position: Position,
    },
    TestFinished {
        name: &'a str,
        duration: i64,
    },
    TestFailed {
        name: &'a str,
        message: &'a str,
        duration: i64,
    },
    TestIgnored {
        name: &'a str,
        message: &'a str,
        duration: i64,
    },
}
