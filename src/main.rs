use anyhow::{anyhow, bail, Result};
use std::{iter::Peekable, str::Chars};
use tokenizer::Tokenizer;

use crate::tokenizer::Token;

mod tokenizer;

fn main() {
    println!("Hello, world!");
}

struct Id(String);

enum InstructionOrList {
    Instruction(Instruction),
    List(Vec<Instruction>),
}

struct InstructionOrListIterator<'a> {
    tokenizer: Tokenizer<'a>,
}

impl<'a> InstructionOrListIterator<'a> {
    pub fn new(chars: Peekable<Chars<'a>>) -> Self {
        Self {
            tokenizer: Tokenizer::new(chars),
        }
    }

    fn internal_next(&mut self) -> Result<Option<InstructionOrList>> {
        match self.tokenizer.next() {
            None => Ok(None),
            Some(Err(e)) => Err(e),
            Some(Ok(Token::String(string))) => todo!("Parse single instruction"),
            Some(Ok(Token::List(list))) => {
                let mut result = Vec::new();
                for token in list {
                    let Token::List(list) = token else {
                        bail!("Expected a list");
                    };
                    match list.get(1) {
                        None => bail!("Expected a instruction"),
                        Some(Token::String(ref cmd)) if cmd == "import" => {
                            result.push(Instruction::Import {
                                id: Id(list
                                    .swap_remove(0)
                                    .ok_or(anyhow!("Expectd an ID"))?
                                    .get_string()?),
                                path: list.get(2).ok_or(anyhow!("Expectd a path"))?.get_string()?,
                            })
                        }
                        any => bail!("Instruction {any:?} not recognized"),
                    }
                }
                Ok(Some(InstructionOrList::List(result)))
            }
        }
    }
}

impl<'a> Iterator for InstructionOrListIterator<'a> {
    type Item = Result<InstructionOrList>;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_next().transpose()
    }
}

enum Instruction {
    Import {
        id: Id,
        path: String,
    },
    Make {
        id: Id,
        instance: String,
        class: String,
        args: String,
    },
    Call {
        id: Id,
        instance: String,
        function: String,
        args: String,
    },
    CallAndAssign {
        id: Id,
        symbol: String,
        instance: String,
        function: String,
        args: Vec<String>,
    },
    Assign {
        id: Id,
        symbol: String,
        value: String,
    },
}
