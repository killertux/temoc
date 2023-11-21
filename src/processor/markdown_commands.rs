use anyhow::{anyhow, bail, Result};
use convert_case::{Case, Casing};
use markdown::{mdast::Node, unist::Position as MPosition};
use std::fmt::Display;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownCommand {
    Import { path: String, position: Position },
    DecisionTable { class: Class, table: Vec<TableRow> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    line: usize,
    column: usize,
}

impl Position {
    #[allow(dead_code)]
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

impl From<MPosition> for Position {
    fn from(value: MPosition) -> Self {
        Self {
            line: value.start.line,
            column: value.start.column,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodName(pub String, pub Position);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class(pub String, pub Position);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value(pub String, pub Position);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub setters: Vec<(MethodName, Value)>,
    pub getters: Vec<(MethodName, Value)>,
}

enum MethodType {
    Getter,
    Setter,
    Commentary,
}

pub fn get_commands_from_markdown(markdown: Node) -> Result<Vec<MarkdownCommand>> {
    let mut result = Vec::new();
    match markdown {
        Node::Root(root) => {
            let mut executing_test = None;
            for node in root.children {
                if let Some(test_class) = executing_test {
                    let Node::Table(table) = node else {
                        bail!("Expected a test table")
                    };
                    let mut rows = Vec::new();
                    let mut methods = Vec::new();
                    for row in table.children {
                        let Node::TableRow(row) = row else {
                            bail!("Expected a table row")
                        };
                        if methods.is_empty() {
                            for cell in row.children {
                                let Node::TableCell(mut cell) = cell else {
                                    bail!("Expected a table cell")
                                };
                                let Node::Text(text) = cell.children.remove(0) else {
                                    bail!("Expected a text")
                                };
                                let position =
                                    text.position.ok_or(anyhow!("Expected position"))?.into();
                                let text = text.value.trim();
                                if text.starts_with("#") {
                                    methods.push((
                                        MethodName(text.into(), position),
                                        MethodType::Commentary,
                                    ));
                                    continue;
                                }
                                match text.split_once('?') {
                                    Some((getter_name, _)) => methods.push((
                                        MethodName(getter_name.to_case(Case::Camel), position),
                                        MethodType::Getter,
                                    )),
                                    _ => {
                                        if text.starts_with("set") {
                                            methods.push((
                                                MethodName(text.to_case(Case::Camel), position),
                                                MethodType::Setter,
                                            ))
                                        } else {
                                            methods.push((
                                                MethodName(
                                                    format!("set {text}").to_case(Case::Camel),
                                                    position,
                                                ),
                                                MethodType::Setter,
                                            ))
                                        }
                                    }
                                }
                            }
                            continue;
                        }
                        let mut table_row = TableRow {
                            setters: Vec::new(),
                            getters: Vec::new(),
                        };
                        for (i, cell) in row.children.into_iter().enumerate() {
                            let Node::TableCell(mut cell) = cell else {
                                bail!("Expected a table cell")
                            };
                            let Node::Text(text) = cell.children.remove(0) else {
                                bail!("Expected a text")
                            };
                            let position =
                                text.position.ok_or(anyhow!("Expected position"))?.into();
                            let text = text.value.trim();
                            match methods.get(i) {
                                None => bail!("Wrong number of columns in row"),
                                Some((method_name, MethodType::Getter)) => table_row
                                    .getters
                                    .push((method_name.clone(), Value(text.to_string(), position))),
                                Some((method_name, MethodType::Setter)) => table_row
                                    .setters
                                    .push((method_name.clone(), Value(text.to_string(), position))),
                                Some((_, MethodType::Commentary)) => {}
                            }
                        }
                        rows.push(table_row);
                    }
                    result.push(MarkdownCommand::DecisionTable {
                        class: test_class,
                        table: rows,
                    });
                    executing_test = None;
                    continue;
                }
                match node {
                    Node::Definition(definition)
                        if definition.url == "#" && definition.identifier == "//" =>
                    {
                        let Some(command) = definition.title else {
                            continue;
                        };
                        let position = definition
                            .position
                            .ok_or(anyhow!("Expected position"))?
                            .into();
                        match command.split_once(' ') {
                            Some(("import", import)) => result.push(MarkdownCommand::Import {
                                path: import.to_string(),
                                position,
                            }),
                            Some(("test", test_class)) => {
                                executing_test = Some(Class(test_class.to_string(), position));
                            }
                            _ => continue,
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => bail!("Expected root markdown document"),
    }
    Ok(result)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn calculator() -> Result<()> {
        let commands = get_commands_from_markdown(parse_markdown(
            r#"
## Calculator

This is a calculator example

[//]: # (import Fixtures)

[//]: # (test Calculator)

| a  | b   | sum? |
|----|-----|------|
| 1  | 2   | 3    |
| 2  | 2   | 4    |
            "#,
        ))?;
        assert_eq!(
            vec![
                MarkdownCommand::Import {
                    path: "Fixtures".into(),
                    position: Position::new(6, 1)
                },
                MarkdownCommand::DecisionTable {
                    class: Class("Calculator".into(), Position::new(8, 1)),
                    table: vec![
                        TableRow {
                            setters: vec![
                                (
                                    MethodName("setA".into(), Position::new(10, 3)),
                                    Value("1".into(), Position::new(12, 3))
                                ),
                                (
                                    MethodName("setB".into(), Position::new(10, 8)),
                                    Value("2".into(), Position::new(12, 8))
                                )
                            ],
                            getters: vec![(
                                MethodName("sum".into(), Position::new(10, 14)),
                                Value("3".into(), Position::new(12, 14))
                            )]
                        },
                        TableRow {
                            setters: vec![
                                (
                                    MethodName("setA".into(), Position::new(10, 3)),
                                    Value("2".into(), Position::new(13, 3))
                                ),
                                (
                                    MethodName("setB".into(), Position::new(10, 8)),
                                    Value("2".into(), Position::new(13, 8))
                                )
                            ],
                            getters: vec![(
                                MethodName("sum".into(), Position::new(10, 14)),
                                Value("4".into(), Position::new(13, 14))
                            )]
                        }
                    ]
                }
            ],
            commands
        );
        Ok(())
    }

    #[test]
    fn spaces_into_camel_case_and_handle_set() -> Result<()> {
        let commands = get_commands_from_markdown(parse_markdown(
            r#"
[//]: # (test Calculator)

| set a | a getter? |
|-------|-----------|
| value | expected  |
            "#,
        ))?;
        assert_eq!(
            vec![MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(2, 1)),
                table: vec![TableRow {
                    setters: vec![(
                        MethodName("setA".into(), Position::new(4, 3)),
                        Value("value".into(), Position::new(6, 3))
                    )],
                    getters: vec![(
                        MethodName("aGetter".into(), Position::new(4, 11)),
                        Value("expected".into(), Position::new(6, 11))
                    )]
                },]
            }],
            commands
        );
        Ok(())
    }

    #[test]
    fn tables_without_test_header_should_ignore() -> Result<()> {
        let commands = get_commands_from_markdown(parse_markdown(
            r#"
| set a | a getter? |
|-------|-----------|
| value | expected  |
            "#,
        ))?;
        assert_eq!(Vec::<MarkdownCommand>::new(), commands);
        Ok(())
    }

    #[test]
    fn ignore_commentaries() -> Result<()> {
        let commands = get_commands_from_markdown(parse_markdown(
            r#"
[//]: # (test Calculator)

| a     | # comment | b?        |
|-------|-----------|-----------|  
| value | comment   | expected  |
            "#,
        ))?;
        assert_eq!(
            vec![MarkdownCommand::DecisionTable {
                class: Class("Calculator".into(), Position::new(2, 1)),
                table: vec![TableRow {
                    setters: vec![(
                        MethodName("setA".into(), Position::new(4, 3)),
                        Value("value".into(), Position::new(6, 3))
                    )],
                    getters: vec![(
                        MethodName("b".into(), Position::new(4, 23)),
                        Value("expected".into(), Position::new(6, 23))
                    )]
                },]
            }],
            commands
        );
        Ok(())
    }

    fn parse_markdown(markdown: &str) -> Node {
        markdown::to_mdast(markdown, &markdown::ParseOptions::gfm())
            .expect("Error parsing markdown")
    }
}
