use anyhow::{anyhow, bail, Result};
use std::{iter::Peekable, str::Chars};

#[derive(Debug, PartialEq, Eq)]
pub enum Token {
    String(String),
    List(Vec<Token>),
}

impl Token {
    pub fn get_string(self) -> Result<String> {
        match self {
            Token::String(str) => Ok(str),
            _ => bail!("Expected a string"),
        }
    }
}

pub struct Tokenizer<'a> {
    chars: Peekable<Chars<'a>>,
}

impl<'a> Tokenizer<'a> {
    pub fn new(chars: Peekable<Chars<'a>>) -> Self {
        Self { chars }
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Result<Token>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.chars.peek() {
            None => None,
            _ => Some(Token::from_chars(&mut self.chars)),
        }
    }
}

impl Token {
    fn from_chars<'a>(chars: &mut Peekable<Chars<'a>>) -> Result<Self> {
        let mut acc = String::new();
        acc.reserve_exact(6);
        loop {
            match chars.next() {
                Some(x) if x.is_numeric() => acc.push(x),
                Some(':') if chars.peek() == Some(&'[') => {
                    return Ok(Token::List(Self::parse_list(dbg!(acc).parse()?, chars)?))
                }
                Some(':') => {
                    return Ok(Token::String(Self::parse_string(acc.parse()?, chars)?));
                }
                _ => bail!("Expected a digit"),
            }
        }
    }

    fn parse_string(size: usize, chars: &mut Peekable<Chars<'_>>) -> Result<String> {
        let result: String = chars.take(size).collect();
        if result.len() != size {
            bail!(
                "String is smaller than expected. Expected {size} gor {}",
                result.len()
            )
        }
        Ok(result)
    }

    fn parse_list(size: usize, chars: &mut Peekable<Chars<'_>>) -> Result<Vec<Token>> {
        let part: String = chars.skip(1).take(size - 1).collect();
        if part.len() != size - 1 {
            bail!(
                "String is smaller than expected. Expected {} got {}",
                size - 1,
                part.len()
            )
        }
        let (n_elements, rest) = part
            .split_once(':')
            .ok_or(anyhow!("Expected size of list"))?;
        let n_elements: usize = n_elements.parse()?;
        let mut result = Vec::new();
        result.reserve_exact(n_elements);
        let mut chars = rest.chars().peekable();
        loop {
            if result.len() == n_elements {
                break;
            }
            result.push(Self::from_chars(&mut chars)?);
            if let None = chars.next_if_eq(&':') {
                bail!("Expected separator :")
            }
        }
        if let None = chars.next_if_eq(&']') {
            bail!("Expected list terminator")
        }

        Ok(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_string() -> Result<()> {
        assert_eq!(
            Token::String("Example".into()),
            Token::from_chars(&mut "000007:Example".chars().peekable())?
        );
        Ok(())
    }

    #[test]
    fn test_empty_string() -> Result<()> {
        assert_eq!(
            Token::String(String::new()),
            Token::from_chars(&mut "000000:".chars().peekable())?
        );
        Ok(())
    }

    #[test]
    fn test_wrong_size_of_string() -> Result<()> {
        assert_eq!(
            Err("String is smaller than expected. Expected 7 gor 6".into()),
            Token::from_chars(&mut "000007:Exampl".chars().peekable())
                .map_err(|err| err.to_string())
        );
        Ok(())
    }

    #[test]
    fn expect_a_digit() -> Result<()> {
        assert_eq!(
            Err("Expected a digit".into()),
            Token::from_chars(&mut "".chars().peekable()).map_err(|err| err.to_string())
        );
        assert_eq!(
            Err("Expected a digit".into()),
            Token::from_chars(&mut "abc".chars().peekable()).map_err(|err| err.to_string())
        );
        Ok(())
    }

    #[test]
    fn test_list() -> Result<()> {
        assert_eq!(
            Token::List(vec![
                Token::String("hello".into()),
                Token::String("world".into())
            ]),
            Token::from_chars(
                &mut "000035:[000002:000005:hello:000005:world:]"
                    .chars()
                    .peekable()
            )?
        );
        Ok(())
    }

    #[test]
    fn test_complex_list() -> Result<()> {
        assert_eq!(
            Token::List(vec![
                Token::String("hello".into()),
                Token::String("world".into()),
                Token::List(vec![Token::String("silver".into())])
            ]),
            Token::from_chars(
                &mut "000066:[000003:000005:hello:000005:world:000023:[000001:000006:silver:]:]"
                    .chars()
                    .peekable()
            )?
        );
        Ok(())
    }

    #[test]
    fn test_tokenizer() -> Result<()> {
        assert_eq!(
            vec![Token::String("hello".into()), Token::String("world".into()),],
            Tokenizer::new("000005:hello000005:world".chars().peekable())
                .collect::<Result<Vec<Token>>>()?
        );
        Ok(())
    }
}
