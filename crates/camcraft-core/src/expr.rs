//! Expressions a setting is worked out from, as Fusion's parameter expressions
//! have them: `tool_diameter * 0.4`,
//! `Math.min(tool_fluteLength * 0.75; tool_diameter * 2.5)`,
//! `tool_type == 'ball end mill' ? tolerance * 4 : 1mm`.
//!
//! - Numbers, with a length unit after one if wanted (`2mm`, `0.1in`, `1cm`,
//!   `1m`, `1ft`): lengths are worked out in millimetres, so a number without
//!   one is millimetres, or whatever the setting itself is in (degrees, mm/min).
//! - Text in single or double quotes, compared with `==` and `!=`.
//! - `+ - * /`, `^` for a power, `( )`; `== != < <= > >=`; `&& || !`; and
//!   `condition ? then : else`.
//! - Fusion's functions, their arguments split by `;` (or `,`): `Math.min`,
//!   `Math.max` (and Fusion's `Math.minimum`, `Math.maximum`), `Math.abs`,
//!   `Math.sqrt`, `Math.floor`, `Math.ceil`, `Math.round`, `Math.floorto`,
//!   `Math.ceilto`, `Math.roundto` (to a step), `Math.sin`, `Math.cos`,
//!   `Math.tan`, `Math.asin`, `Math.acos`, `Math.atan` (in radians),
//!   `Math.pow`; and `Math.PI`, `true`, `false`.
//! - Any other name is a value the caller gives ([`Expr::number`]), as
//!   `tool_diameter` or `stepover`.

use std::fmt;

/// What part of an expression comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number: a length in millimetres, or the setting's own unit.
    Number(f64),
    /// Text, as a tool's type.
    Text(String),
    /// Whether a comparison holds.
    Bool(bool),
}

/// Why an expression cannot be worked out, and where in it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (at character {at})")]
pub struct ExprError {
    /// What is wrong.
    pub message: String,
    /// Where, counting characters from one.
    pub at: usize,
}

/// An expression, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    node: Node,
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Number(f64),
    Text(String),
    Name(String, usize),
    Unary(Unary, Box<Node>),
    Binary(Binary, Box<Node>, Box<Node>),
    Choose(Box<Node>, Box<Node>, Box<Node>),
    Call(String, Vec<Node>, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unary {
    Minus,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Binary {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// Millimetres in each length unit a number may be followed by.
const UNITS: [(&str, f64); 5] = [
    ("mm", 1.0),
    ("cm", 10.0),
    ("m", 1000.0),
    ("in", camcraft_geom::MM_PER_INCH),
    ("ft", 12.0 * camcraft_geom::MM_PER_INCH),
];

impl Expr {
    /// Read `text`.
    pub fn parse(text: &str) -> Result<Self, ExprError> {
        let mut p = Parser {
            chars: text.chars().collect(),
            at: 0,
        };
        let node = p.choose()?;
        p.space();
        if p.at < p.chars.len() {
            return Err(p.error("expected an operator or the end"));
        }
        Ok(Self { node })
    }

    /// What it comes to, as a finite number, with each name's value from
    /// `value`: `None` for a name it does not know.
    pub fn number(&self, value: &dyn Fn(&str) -> Option<Value>) -> Result<f64, ExprError> {
        match eval(&self.node, value)? {
            Value::Number(n) if n.is_finite() => Ok(n),
            Value::Number(_) => Err(ExprError {
                message: "it comes to no finite number".into(),
                at: 1,
            }),
            Value::Text(_) | Value::Bool(_) => Err(ExprError {
                message: "it comes to text or a yes or no, not a number".into(),
                at: 1,
            }),
        }
    }
}

/// Read `text` and work it out, as [`Expr::parse`] and [`Expr::number`].
pub fn evaluate(text: &str, value: &dyn Fn(&str) -> Option<Value>) -> Result<f64, ExprError> {
    Expr::parse(text)?.number(value)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn error(&self, message: impl Into<String>) -> ExprError {
        ExprError {
            message: message.into(),
            at: self.at + 1,
        }
    }

    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.space();
        self.chars.get(self.at).copied()
    }

    /// Take `token` if it comes next.
    fn eat(&mut self, token: &str) -> bool {
        self.space();
        let len = token.chars().count();
        let next = self.chars.get(self.at..self.at + len);
        if next.is_some_and(|s| s.iter().copied().eq(token.chars())) {
            self.at += len;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &str) -> Result<(), ExprError> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{token}`")))
        }
    }

    /// `a ? b : c`, the loosest.
    fn choose(&mut self) -> Result<Node, ExprError> {
        let test = self.or()?;
        if !self.eat("?") {
            return Ok(test);
        }
        let then = self.choose()?;
        self.expect(":")?;
        let otherwise = self.choose()?;
        Ok(Node::Choose(
            Box::new(test),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    /// Operators of one tightness, left to right, each spelled as in `ops`.
    fn left(
        &mut self,
        ops: &[(&str, Binary)],
        next: fn(&mut Self) -> Result<Node, ExprError>,
    ) -> Result<Node, ExprError> {
        let mut node = next(self)?;
        'more: loop {
            for (token, op) in ops {
                if self.eat(token) {
                    node = Node::Binary(*op, Box::new(node), Box::new(next(self)?));
                    continue 'more;
                }
            }
            return Ok(node);
        }
    }

    fn or(&mut self) -> Result<Node, ExprError> {
        self.left(&[("||", Binary::Or)], Self::and)
    }

    fn and(&mut self) -> Result<Node, ExprError> {
        self.left(&[("&&", Binary::And)], Self::compare)
    }

    fn compare(&mut self) -> Result<Node, ExprError> {
        // Two characters before one, so `<=` is not read as `<`.
        self.left(
            &[
                ("==", Binary::Eq),
                ("!=", Binary::Ne),
                ("<=", Binary::Le),
                (">=", Binary::Ge),
                ("<", Binary::Lt),
                (">", Binary::Gt),
            ],
            Self::sum,
        )
    }

    fn sum(&mut self) -> Result<Node, ExprError> {
        self.left(&[("+", Binary::Add), ("-", Binary::Sub)], Self::product)
    }

    fn product(&mut self) -> Result<Node, ExprError> {
        self.left(&[("*", Binary::Mul), ("/", Binary::Div)], Self::unary)
    }

    fn unary(&mut self) -> Result<Node, ExprError> {
        if self.eat("-") {
            return Ok(Node::Unary(Unary::Minus, Box::new(self.unary()?)));
        }
        if self.eat("+") {
            return self.unary();
        }
        // Not `!=`, which only follows something.
        if self.peek() == Some('!') {
            self.at += 1;
            return Ok(Node::Unary(Unary::Not, Box::new(self.unary()?)));
        }
        self.power()
    }

    /// `a ^ b`, right to left, binding tighter than a sign before it.
    fn power(&mut self) -> Result<Node, ExprError> {
        let base = self.primary()?;
        if self.eat("^") {
            let exponent = self.unary()?;
            return Ok(Node::Binary(
                Binary::Pow,
                Box::new(base),
                Box::new(exponent),
            ));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Node, ExprError> {
        match self.peek() {
            None => Err(self.error("expected a value")),
            Some('(') => {
                self.at += 1;
                let inner = self.choose()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(q @ ('\'' | '"')) => {
                let start = self.at;
                self.at += 1;
                let mut text = String::new();
                loop {
                    match self.chars.get(self.at) {
                        Some(c) if *c == q => break,
                        Some(c) => text.push(*c),
                        None => {
                            self.at = start;
                            return Err(self.error("text left unclosed"));
                        }
                    }
                    self.at += 1;
                }
                self.at += 1;
                Ok(Node::Text(text))
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_alphabetic() || c == '_' => self.name(),
            Some(c) => Err(self.error(format!("`{c}` was not expected here"))),
        }
    }

    fn number(&mut self) -> Result<Node, ExprError> {
        let start = self.at;
        let digits = |p: &mut Self| {
            while p.chars.get(p.at).is_some_and(char::is_ascii_digit) {
                p.at += 1;
            }
        };
        digits(self);
        if self.chars.get(self.at) == Some(&'.') {
            self.at += 1;
            digits(self);
        }
        // An exponent, as 1e-3: only where digits follow it, so `2e` is not one.
        if matches!(self.chars.get(self.at), Some('e' | 'E')) {
            let mut k = self.at + 1;
            if matches!(self.chars.get(k), Some('+' | '-')) {
                k += 1;
            }
            if self.chars.get(k).is_some_and(char::is_ascii_digit) {
                self.at = k;
                digits(self);
            }
        }
        let text: String = self
            .chars
            .get(start..self.at)
            .unwrap_or_default()
            .iter()
            .collect();
        let n: f64 = text.parse().map_err(|_| ExprError {
            message: format!("`{text}` is not a number"),
            at: start + 1,
        })?;
        Ok(Node::Number(n * self.unit()))
    }

    /// The length unit after a number, as millimetres per one, or 1 for
    /// none.
    fn unit(&mut self) -> f64 {
        let word_end = |p: &Self, from: usize| {
            let mut k = from;
            while p
                .chars
                .get(k)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                k += 1;
            }
            k
        };
        // A space between is allowed: a name cannot follow a number anyway.
        let mut from = self.at;
        while self.chars.get(from).is_some_and(|c| c.is_whitespace()) {
            from += 1;
        }
        let end = word_end(self, from);
        let word: String = self
            .chars
            .get(from..end)
            .unwrap_or_default()
            .iter()
            .collect();
        match UNITS.iter().find(|(name, _)| *name == word) {
            Some((_, mm)) => {
                self.at = end;
                *mm
            }
            None => 1.0,
        }
    }

    fn name(&mut self) -> Result<Node, ExprError> {
        let start = self.at;
        while self
            .chars
            .get(self.at)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        {
            self.at += 1;
        }
        let name: String = self
            .chars
            .get(start..self.at)
            .unwrap_or_default()
            .iter()
            .collect();
        if !self.eat("(") {
            return Ok(Node::Name(name, start + 1));
        }
        let mut args = Vec::new();
        if !self.eat(")") {
            loop {
                args.push(self.choose()?);
                if self.eat(";") || self.eat(",") {
                    continue;
                }
                self.expect(")")?;
                break;
            }
        }
        Ok(Node::Call(name, args, start + 1))
    }
}

/// The error for `node`, where it starts if that is known.
fn fault(message: impl Into<String>, at: usize) -> ExprError {
    ExprError {
        message: message.into(),
        at,
    }
}

fn number_of(v: &Value, at: usize) -> Result<f64, ExprError> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Text(t) => Err(fault(format!("'{t}' is text, not a number"), at)),
    }
}

fn truth(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0,
        Value::Text(t) => !t.is_empty(),
    }
}

/// Where `node` starts, as far as it says.
fn start(node: &Node) -> usize {
    match node {
        Node::Name(_, at) | Node::Call(_, _, at) => *at,
        Node::Unary(_, n) | Node::Binary(_, n, _) | Node::Choose(n, _, _) => start(n),
        Node::Number(_) | Node::Text(_) => 1,
    }
}

fn eval(node: &Node, value: &dyn Fn(&str) -> Option<Value>) -> Result<Value, ExprError> {
    let num = |n: &Node| eval(n, value).and_then(|v| number_of(&v, start(n)));
    Ok(match node {
        Node::Number(n) => Value::Number(*n),
        Node::Text(t) => Value::Text(t.clone()),
        Node::Name(name, at) => match name.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "Math.PI" | "PI" => Value::Number(std::f64::consts::PI),
            _ => value(name).ok_or_else(|| fault(format!("there is no value `{name}`"), *at))?,
        },
        Node::Unary(Unary::Minus, n) => Value::Number(-num(n)?),
        Node::Unary(Unary::Not, n) => Value::Bool(!truth(&eval(n, value)?)),
        Node::Choose(test, then, otherwise) => {
            if truth(&eval(test, value)?) {
                eval(then, value)?
            } else {
                eval(otherwise, value)?
            }
        }
        Node::Binary(op, a, b) => match op {
            Binary::And => Value::Bool(truth(&eval(a, value)?) && truth(&eval(b, value)?)),
            Binary::Or => Value::Bool(truth(&eval(a, value)?) || truth(&eval(b, value)?)),
            Binary::Eq | Binary::Ne => {
                // Exactly, as Fusion's `==` compares.
                #[allow(clippy::float_cmp)]
                let same = match (eval(a, value)?, eval(b, value)?) {
                    (Value::Text(x), Value::Text(y)) => x == y,
                    (Value::Text(_), _) | (_, Value::Text(_)) => false,
                    (x, y) => number_of(&x, 1)? == number_of(&y, 1)?,
                };
                Value::Bool(same == (*op == Binary::Eq))
            }
            Binary::Div => {
                let (x, y) = (num(a)?, num(b)?);
                if y == 0.0 {
                    return Err(fault("division by zero", start(b)));
                }
                Value::Number(x / y)
            }
            Binary::Add => Value::Number(num(a)? + num(b)?),
            Binary::Sub => Value::Number(num(a)? - num(b)?),
            Binary::Mul => Value::Number(num(a)? * num(b)?),
            Binary::Pow => Value::Number(num(a)?.powf(num(b)?)),
            Binary::Lt => Value::Bool(num(a)? < num(b)?),
            Binary::Le => Value::Bool(num(a)? <= num(b)?),
            Binary::Gt => Value::Bool(num(a)? > num(b)?),
            Binary::Ge => Value::Bool(num(a)? >= num(b)?),
        },
        Node::Call(name, args, at) => call(name, args, *at, value)?,
    })
}

fn call(
    name: &str,
    args: &[Node],
    at: usize,
    value: &dyn Fn(&str) -> Option<Value>,
) -> Result<Value, ExprError> {
    let nums = args
        .iter()
        .map(|a| eval(a, value).and_then(|v| number_of(&v, start(a))))
        .collect::<Result<Vec<f64>, ExprError>>()?;
    let short = name.strip_prefix("Math.").unwrap_or(name);
    let want = |n: usize| -> Result<(), ExprError> {
        if nums.len() == n {
            Ok(())
        } else {
            Err(fault(
                format!("{name} takes {n} value(s), not {}", nums.len()),
                at,
            ))
        }
    };
    let one = |f: fn(f64) -> f64| -> Result<f64, ExprError> {
        want(1)?;
        Ok(f(nums.first().copied().unwrap_or_default()))
    };
    let to_step = |f: fn(f64) -> f64| -> Result<f64, ExprError> {
        want(2)?;
        let (x, step) = (
            nums.first().copied().unwrap_or_default(),
            nums.get(1).copied().unwrap_or_default(),
        );
        if step <= 0.0 {
            return Err(fault(format!("{name}'s step must be more than zero"), at));
        }
        Ok(f(x / step) * step)
    };
    let n = match short {
        "min" | "minimum" | "max" | "maximum" => {
            if nums.is_empty() {
                return Err(fault(format!("{name} takes at least one value"), at));
            }
            let low = short.starts_with("min");
            nums.iter()
                .copied()
                .reduce(|a, b| if low { a.min(b) } else { a.max(b) })
                .unwrap_or_default()
        }
        "abs" => one(f64::abs)?,
        "sqrt" => {
            if nums.first().is_some_and(|x| *x < 0.0) {
                return Err(fault("the square root of a negative number", at));
            }
            one(f64::sqrt)?
        }
        "floor" => one(f64::floor)?,
        "ceil" => one(f64::ceil)?,
        "round" => one(f64::round)?,
        "floorto" => to_step(f64::floor)?,
        "ceilto" => to_step(f64::ceil)?,
        "roundto" => to_step(f64::round)?,
        "sin" => one(f64::sin)?,
        "cos" => one(f64::cos)?,
        "tan" => one(f64::tan)?,
        "asin" => one(f64::asin)?,
        "acos" => one(f64::acos)?,
        "atan" => one(f64::atan)?,
        "pow" => {
            want(2)?;
            nums.first()
                .copied()
                .unwrap_or_default()
                .powf(nums.get(1).copied().unwrap_or_default())
        }
        _ => return Err(fault(format!("there is no function `{name}`"), at)),
    };
    Ok(Value::Number(n))
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(t) => write!(f, "'{t}'"),
            Self::Bool(b) => write!(f, "{b}"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests;
