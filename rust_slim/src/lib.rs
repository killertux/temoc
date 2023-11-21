use anyhow::Result;

pub trait SlimFixture {
    fn begin_table(&mut self) -> Result<()> {
        Ok(())
    }
    fn reset(&mut self) -> Result<()> {
        Ok(())
    }
    fn execute(&mut self) -> Result<()> {
        Ok(())
    }
    fn end_table(&mut self) -> Result<()> {
        Ok(())
    }
    fn execute_method(&mut self, method: &str) -> Result<Option<String>>;
}

pub trait ClassPath {
    fn class_path() -> String;
}