use crate::processor::Position;
use anyhow::Result;
pub use team_city::TeamCityTestLogger;

mod team_city;

pub trait TestLogger {
    fn number_of_tests(&mut self, n: usize) -> anyhow::Result<()>;
    fn file_started(&mut self, name: &str) -> Result<()>;
    fn file_finished(&mut self) -> Result<()>;
    fn row_started(
        &mut self,
        decision_table: &str,
        row_number: usize,
        position: Position,
    ) -> Result<()>;
    fn row_failed(&mut self, message: &str) -> Result<()>;
    fn row_snoozed(&mut self, message: &str) -> Result<()>;
    fn row_finished(&mut self) -> Result<()>;
}

pub struct NullLogger {}

impl TestLogger for NullLogger {
    fn number_of_tests(&mut self, _n: usize) -> anyhow::Result<()> {
        Ok(())
    }
    fn file_started(&mut self, _name: &str) -> Result<()> {
        Ok(())
    }
    fn file_finished(&mut self) -> Result<()> {
        Ok(())
    }
    fn row_started(
        &mut self,
        _decision_table: &str,
        _row_number: usize,
        _location: Position,
    ) -> Result<()> {
        Ok(())
    }

    fn row_failed(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }

    fn row_snoozed(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }

    fn row_finished(&mut self) -> Result<()> {
        Ok(())
    }
}
