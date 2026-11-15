//! Translation of ECMA-262 pattern sources into `regex` crate expressions.
//!
//! Every JSON Schema `pattern` zerx compiles passes through here. The result is
//! a deliberate conservative subset of ECMA-262 with the `u` flag: a construct
//! whose meaning differs between the two dialects matches only what both
//! accept, and a construct that cannot be held is refused rather than
//! approximated.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EcmaError {
    /// Invalid ECMA-262: no JavaScript engine would accept this source.
    Malformed(String),
    /// Valid ECMA-262 that the translation cannot represent.
    Unsupported(String),
}

impl fmt::Display for EcmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EcmaError::Malformed(msg) | EcmaError::Unsupported(msg) => f.write_str(msg),
        }
    }
}

/// Translate then build the `regex` crate expression for `src`.
pub(crate) fn compile(src: &str) -> Result<regex::Regex, EcmaError> {
    let translated = translate(src)?;
    regex::Regex::new(&translated).map_err(|e| EcmaError::Unsupported(e.to_string()))
}

/// Translate an ECMA-262 (`u` flag) source into a `regex` crate expression.
pub(crate) fn translate(src: &str) -> Result<String, EcmaError> {
    let chars: Vec<char> = src.chars().collect();
    let (capture_total, existing_names) = prescan(&chars);
    let mut parser = Parser {
        chars: &chars,
        pos: 0,
        depth: 0,
        alt_stack: vec![0],
        declared_names: Vec::new(),
        capture_total,
        existing_names,
        w7_unsupported: false,
    };
    let out = parser.parse_disjunction()?;
    if parser.pos < parser.chars.len() {
        // Only an unbalanced `)` can survive the root disjunction.
        return Err(EcmaError::Malformed("unbalanced `)`".to_string()));
    }
    if parser.w7_unsupported {
        return Err(EcmaError::Unsupported(
            "an unescaped `[` inside a class is not supported".to_string(),
        ));
    }
    Ok(out)
}

/// The `regex` crate's own nesting limit; staying at or below it keeps a deeply
/// nested source from overflowing the stack.
const MAX_DEPTH: usize = 250;

/// ECMA-262 WhiteSpace + LineTerminator, minus U+FEFF.
///
/// No single `regex` class carries this set: Rust's `\s` is `White_Space`, which
/// adds U+0085 and lacks U+FEFF. Written out because intersection semantics need
/// the ECMA set and the Rust set to be distinguishable.
const ECMA_SPACE: &str =
    r"\x{9}-\x{D}\x{20}\x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}";

#[derive(Clone, Copy, PartialEq, Eq)]
enum TermKind {
    Atom,
    Assertion,
}

enum ClassAtom {
    /// A single code point, usable as a range endpoint.
    Char(u32),
    /// A class escape such as `\d`; not usable as a range endpoint.
    Escape(String),
}

impl ClassAtom {
    fn emitted(self) -> String {
        match self {
            ClassAtom::Char(cp) => emit_literal(char::from_u32(cp).expect("valid scalar value")),
            ClassAtom::Escape(s) => s,
        }
    }
}

fn emit_literal(c: char) -> String {
    if c.is_alphanumeric() || !c.is_ascii() {
        c.to_string()
    } else {
        format!(r"\x{{{:X}}}", c as u32)
    }
}

fn is_syntax_char(c: char) -> bool {
    matches!(
        c,
        '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/'
    )
}

/// Emit one of `\d \D \w \W \s \S`.
///
/// Outside a class and in a positive class the emitted set is the *intersection*
/// of the two dialect readings; inside a negated class it is their *union*, so
/// that the class's own complement is the intersection.
fn class_escape(c: char, negated: bool) -> String {
    match (c, negated) {
        ('d', false) => "[0-9]".to_string(),
        ('d', true) => r"\d".to_string(),
        ('D', false) => r"\D".to_string(),
        ('D', true) => "[^0-9]".to_string(),
        ('w', false) => "[A-Za-z0-9_]".to_string(),
        ('w', true) => r"\w".to_string(),
        ('W', false) => r"\W".to_string(),
        ('W', true) => "[^A-Za-z0-9_]".to_string(),
        // `\s`/`\S` come from a fixed ECMA list; by position it is either
        // intersected or united with Rust's `White_Space`, which lacks U+FEFF
        // and adds U+0085. That is why the two forms below differ.
        ('s', false) => format!("[{ECMA_SPACE}]"),
        ('s', true) => r"[\s\x{FEFF}]".to_string(),
        ('S', false) => r"[^\s\x{FEFF}]".to_string(),
        ('S', true) => format!("[^{ECMA_SPACE}]"),
        _ => unreachable!("class_escape called for a non-class escape"),
    }
}

/// Count capturing groups and collect their names before parsing, so that a
/// forward backreference can be classified as naming a group or not.
fn prescan(chars: &[char]) -> (usize, Vec<String>) {
    let mut capture_total = 0usize;
    let mut names: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut in_class = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        match c {
            '[' => {
                in_class = true;
                i += 1;
            }
            '(' => {
                if i + 1 < chars.len() && chars[i + 1] == '?' {
                    if i + 2 < chars.len() && chars[i + 2] == '<' {
                        if i + 3 < chars.len() && (chars[i + 3] == '=' || chars[i + 3] == '!') {
                            i += 3;
                        } else {
                            capture_total += 1;
                            let mut j = i + 3;
                            let mut name = String::new();
                            while j < chars.len() && chars[j] != '>' {
                                name.push(chars[j]);
                                j += 1;
                            }
                            names.push(name);
                            i = j;
                        }
                    } else {
                        i += 2;
                    }
                } else {
                    capture_total += 1;
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    (capture_total, names)
}

struct Parser<'a> {
    chars: &'a [char],
    pos: usize,
    depth: usize,
    /// Alternative index at each disjunction level, used to decide whether two
    /// same-named groups sit in disjoint alternatives.
    alt_stack: Vec<usize>,
    declared_names: Vec<(String, Vec<usize>)>,
    capture_total: usize,
    existing_names: Vec<String>,
    /// Set when an unescaped `[` appears inside a class. W7 is valid ECMA that
    /// the crate would read as a nested class, but only a clean parse may turn
    /// it into `Unsupported`; a Malformed remainder wins.
    w7_unsupported: bool,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_disjunction(&mut self) -> Result<String, EcmaError> {
        let mut out = String::new();
        loop {
            out.push_str(&self.parse_alternative()?);
            if self.eat('|') {
                out.push('|');
                if let Some(last) = self.alt_stack.last_mut() {
                    *last += 1;
                }
            } else {
                break;
            }
        }
        Ok(out)
    }

    fn parse_alternative(&mut self) -> Result<String, EcmaError> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            match c {
                '|' | ')' => break,
                '*' | '+' | '?' | '{' => {
                    return Err(EcmaError::Malformed("nothing to repeat".to_string()));
                }
                ']' => return Err(EcmaError::Malformed("unbalanced `]`".to_string())),
                '}' => return Err(EcmaError::Malformed("unbalanced `}`".to_string())),
                _ => {}
            }
            let (text, kind) = self.parse_term()?;
            out.push_str(&text);
            match kind {
                TermKind::Assertion => {
                    if self.at_quantifier() {
                        return Err(EcmaError::Malformed("nothing to repeat".to_string()));
                    }
                }
                TermKind::Atom => {
                    if self.at_quantifier() {
                        out.push_str(&self.parse_quantifier()?);
                        if self.at_quantifier() {
                            return Err(EcmaError::Malformed("nothing to repeat".to_string()));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn at_quantifier(&self) -> bool {
        matches!(self.peek(), Some('*' | '+' | '?' | '{'))
    }

    fn parse_term(&mut self) -> Result<(String, TermKind), EcmaError> {
        let c = self.peek().expect("caller checked for a term");
        match c {
            '^' => {
                self.pos += 1;
                Ok(("^".to_string(), TermKind::Assertion))
            }
            '$' => {
                self.pos += 1;
                Ok(("$".to_string(), TermKind::Assertion))
            }
            '.' => {
                self.pos += 1;
                Ok((r"[^\n\r\x{2028}\x{2029}]".to_string(), TermKind::Atom))
            }
            '(' => self.parse_group(),
            '[' => {
                let text = self.parse_class()?;
                Ok((text, TermKind::Atom))
            }
            '\\' => self.parse_escape(),
            _ => {
                self.pos += 1;
                Ok((emit_literal(c), TermKind::Atom))
            }
        }
    }

    fn parse_escape(&mut self) -> Result<(String, TermKind), EcmaError> {
        self.pos += 1; // the backslash
        let e = match self.bump() {
            Some(c) => c,
            None => return Err(EcmaError::Malformed("unterminated escape".to_string())),
        };
        match e {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' => Ok((class_escape(e, false), TermKind::Atom)),
            'b' => {
                // The adjacent `(?-u:\b)` is the ASCII boundary, so the pair
                // asserts a boundary under both readings: the intersection of
                // the Unicode and the ASCII boundary.
                Ok((r"\b(?-u:\b)".to_string(), TermKind::Assertion))
            }
            'B' => Ok((r"\B(?-u:\B)".to_string(), TermKind::Assertion)),
            't' => Ok((r"\x{9}".to_string(), TermKind::Atom)),
            'n' => Ok((r"\x{A}".to_string(), TermKind::Atom)),
            'v' => Ok((r"\x{B}".to_string(), TermKind::Atom)),
            'f' => Ok((r"\x{C}".to_string(), TermKind::Atom)),
            'r' => Ok((r"\x{D}".to_string(), TermKind::Atom)),
            'x' => {
                let cp = self.read_hex(2)?;
                Ok((format!(r"\x{{{cp:X}}}"), TermKind::Atom))
            }
            'u' => {
                let cp = self.parse_unicode_escape()?;
                Ok((format!(r"\x{{{cp:X}}}"), TermKind::Atom))
            }
            'c' => Err(EcmaError::Unsupported(
                "`\\c` control escapes are not supported".to_string(),
            )),
            'p' | 'P' => Err(EcmaError::Unsupported(
                "`\\p{...}` property escapes are not supported".to_string(),
            )),
            'k' => self.parse_named_backreference(),
            '0' => {
                if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    Err(EcmaError::Malformed(
                        "octal escapes are not valid in Unicode mode".to_string(),
                    ))
                } else {
                    Err(EcmaError::Unsupported("`\\0` is not supported".to_string()))
                }
            }
            '1'..='9' => {
                let mut n: usize = (e as u8 - b'0') as usize;
                while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
                    n = n.saturating_mul(10).saturating_add(d as usize);
                    self.pos += 1;
                }
                if (1..=self.capture_total).contains(&n) {
                    Err(EcmaError::Unsupported(
                        "backreferences are not supported".to_string(),
                    ))
                } else {
                    Err(EcmaError::Malformed(format!(
                        "backreference `\\{n}` has no matching group"
                    )))
                }
            }
            _ => {
                if is_syntax_char(e) {
                    Ok((emit_literal(e), TermKind::Atom))
                } else {
                    Err(EcmaError::Malformed(format!(
                        "identity escape `\\{e}` is not valid"
                    )))
                }
            }
        }
    }

    fn parse_named_backreference(&mut self) -> Result<(String, TermKind), EcmaError> {
        if !self.eat('<') {
            return Err(EcmaError::Malformed(
                "`\\k` must be followed by `<name>`".to_string(),
            ));
        }
        let name = self.read_until_gt()?;
        if self.existing_names.iter().any(|n| n == &name) {
            Err(EcmaError::Unsupported(
                "backreferences are not supported".to_string(),
            ))
        } else {
            Err(EcmaError::Malformed(format!(
                "backreference `\\k<{name}>` has no matching group"
            )))
        }
    }

    fn read_until_gt(&mut self) -> Result<String, EcmaError> {
        let mut name = String::new();
        loop {
            match self.bump() {
                Some('>') => return Ok(name),
                Some(c) => name.push(c),
                None => return Err(EcmaError::Malformed("unterminated group name".to_string())),
            }
        }
    }

    fn read_hex(&mut self, n: usize) -> Result<u32, EcmaError> {
        let mut value = 0u32;
        for _ in 0..n {
            let c = self
                .peek()
                .ok_or_else(|| EcmaError::Malformed("unterminated hex escape".to_string()))?;
            let digit = c
                .to_digit(16)
                .ok_or_else(|| EcmaError::Malformed("invalid hex escape".to_string()))?;
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn parse_unicode_escape(&mut self) -> Result<u32, EcmaError> {
        if self.eat('{') {
            let mut cp = 0u32;
            let mut any = false;
            while let Some(c) = self.peek() {
                if c == '}' {
                    break;
                }
                let digit = c.to_digit(16).ok_or_else(|| {
                    EcmaError::Malformed("invalid code point in `\\u{...}`".to_string())
                })?;
                cp = cp
                    .checked_mul(16)
                    .and_then(|v| v.checked_add(digit))
                    .ok_or_else(|| {
                        EcmaError::Malformed("`\\u{...}` is out of range".to_string())
                    })?;
                any = true;
                self.pos += 1;
            }
            if !self.eat('}') || !any {
                return Err(EcmaError::Malformed(
                    "invalid `\\u{...}` escape".to_string(),
                ));
            }
            if cp > 0x10FFFF {
                return Err(EcmaError::Malformed(
                    "`\\u{...}` is out of range".to_string(),
                ));
            }
            if (0xD800..=0xDFFF).contains(&cp) {
                return Err(EcmaError::Unsupported(
                    "a lone surrogate cannot be held by a Rust string".to_string(),
                ));
            }
            Ok(cp)
        } else {
            let cp = self.read_hex(4)?;
            if (0xD800..=0xDBFF).contains(&cp) {
                if self.peek() == Some('\\')
                    && self.peek_at(1) == Some('u')
                    && self.peek_at(2) != Some('{')
                {
                    let save = self.pos;
                    self.pos += 2;
                    // An escape-shape error in the second `\u` is invalid ECMA
                    // and must surface as Malformed, not as a lone surrogate.
                    let low = self.read_hex(4)?;
                    if (0xDC00..=0xDFFF).contains(&low) {
                        let combined = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                        return Ok(combined);
                    }
                    // Valid hex but not a low surrogate: two lone surrogates,
                    // which ECMA allows and a Rust string cannot hold.
                    self.pos = save;
                }
                Err(EcmaError::Unsupported(
                    "a lone surrogate cannot be held by a Rust string".to_string(),
                ))
            } else if (0xDC00..=0xDFFF).contains(&cp) {
                Err(EcmaError::Unsupported(
                    "a lone surrogate cannot be held by a Rust string".to_string(),
                ))
            } else {
                Ok(cp)
            }
        }
    }

    fn parse_group(&mut self) -> Result<(String, TermKind), EcmaError> {
        self.pos += 1; // the `(`
        let opener: String;
        if self.eat('?') {
            match self.peek() {
                Some(':') => {
                    self.pos += 1;
                    opener = "(?:".to_string();
                }
                Some('<') => {
                    self.pos += 1;
                    match self.peek() {
                        Some('=') | Some('!') => {
                            return Err(EcmaError::Unsupported(
                                "lookbehind is not supported".to_string(),
                            ));
                        }
                        _ => {
                            let name = self.read_until_gt()?;
                            validate_group_name(&name)?;
                            let path = self.alt_stack.clone();
                            self.register_name(&name, path)?;
                            opener = format!("(?<{name}>");
                        }
                    }
                }
                Some('=') | Some('!') => {
                    return Err(EcmaError::Unsupported(
                        "lookahead is not supported".to_string(),
                    ));
                }
                _ => {
                    if self.looks_like_modifier_group() {
                        return Err(EcmaError::Unsupported(
                            "modifier groups are not supported".to_string(),
                        ));
                    }
                    return Err(EcmaError::Malformed(
                        "unsupported `(?` group syntax".to_string(),
                    ));
                }
            }
        } else {
            opener = "(".to_string();
        }

        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(EcmaError::Unsupported(
                "pattern nesting is too deep".to_string(),
            ));
        }
        self.alt_stack.push(0);
        let inner = self.parse_disjunction()?;
        self.alt_stack.pop();
        self.depth -= 1;
        if !self.eat(')') {
            return Err(EcmaError::Malformed("unbalanced `(`".to_string()));
        }
        Ok((format!("{opener}{inner})"), TermKind::Atom))
    }

    fn looks_like_modifier_group(&self) -> bool {
        let mut i = self.pos;
        let mut any = false;
        while i < self.chars.len() && matches!(self.chars[i], 'i' | 'm' | 's') {
            i += 1;
            any = true;
        }
        if i < self.chars.len() && self.chars[i] == '-' {
            i += 1;
            any = true;
            while i < self.chars.len() && matches!(self.chars[i], 'i' | 'm' | 's') {
                i += 1;
            }
        }
        any && i < self.chars.len() && self.chars[i] == ':'
    }

    fn register_name(&mut self, name: &str, path: Vec<usize>) -> Result<(), EcmaError> {
        for (existing, existing_path) in &self.declared_names {
            if existing == name {
                let disjoint = path.iter().zip(existing_path.iter()).any(|(a, b)| a != b);
                if disjoint {
                    return Err(EcmaError::Unsupported(
                        "duplicate group names are not supported".to_string(),
                    ));
                }
                return Err(EcmaError::Malformed(format!(
                    "duplicate group name `{name}`"
                )));
            }
        }
        self.declared_names.push((name.to_string(), path));
        Ok(())
    }

    fn parse_class(&mut self) -> Result<String, EcmaError> {
        self.pos += 1; // the `[`
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(EcmaError::Unsupported(
                "pattern nesting is too deep".to_string(),
            ));
        }
        let negated = self.eat('^');

        // `[^]` matches any character in ECMA; the crate reads an empty negated
        // class differently, and there is no faithful translation.
        if negated && self.peek() == Some(']') {
            return Err(EcmaError::Unsupported("`[^]` is not supported".to_string()));
        }
        // `[]` is ECMA's empty class: it matches nothing. The crate rejects it,
        // so emit a class spanning every code point and negate it.
        if !negated && self.peek() == Some(']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(r"[^\x{0}-\x{10FFFF}]".to_string());
        }

        let mut members: Vec<String> = Vec::new();
        loop {
            match self.peek() {
                None => {
                    return Err(EcmaError::Malformed(
                        "unterminated character class".to_string(),
                    ))
                }
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                Some(c) => {
                    // The crate reads these as set operations; ECMA reads two
                    // literal characters. Refusing keeps the dialects from
                    // silently disagreeing.
                    if (c == '&' || c == '~') && self.peek_at(1) == Some(c) {
                        return Err(EcmaError::Unsupported(
                            "character class set operations are not supported".to_string(),
                        ));
                    }
                }
            }

            let first = self.parse_class_atom(negated)?;
            let is_range = self.peek() == Some('-')
                && self.peek_at(1).is_some()
                && self.peek_at(1) != Some(']');
            if is_range {
                self.pos += 1; // the `-`
                let second = self.parse_class_atom(negated)?;
                match (first, second) {
                    (ClassAtom::Char(lo), ClassAtom::Char(hi)) => {
                        if lo > hi {
                            return Err(EcmaError::Malformed(
                                "character class range is out of order".to_string(),
                            ));
                        }
                        let lo = emit_literal(
                            char::from_u32(lo).expect("range endpoint is a scalar value"),
                        );
                        let hi = emit_literal(
                            char::from_u32(hi).expect("range endpoint is a scalar value"),
                        );
                        members.push(format!("{lo}-{hi}"));
                    }
                    _ => {
                        return Err(EcmaError::Malformed(
                            "character class range uses a class escape".to_string(),
                        ))
                    }
                }
            } else {
                members.push(first.emitted());
            }
        }
        self.depth -= 1;

        let body = members.concat();
        if negated {
            Ok(format!("[^{body}]"))
        } else {
            Ok(format!("[{body}]"))
        }
    }

    fn parse_class_atom(&mut self, negated: bool) -> Result<ClassAtom, EcmaError> {
        match self.peek() {
            None | Some(']') => Err(EcmaError::Malformed(
                "unterminated character class".to_string(),
            )),
            Some('\\') => self.parse_class_escape(negated),
            Some('[') => {
                // ECMA reads an unescaped `[` inside a class as a literal member;
                // the crate would nest. Emit it as a literal and defer the W7
                // refusal to the end of a clean parse, so a malformed remainder
                // (`[[:alpha:]]`'s lone `]`, say) is still reported as Malformed.
                self.pos += 1;
                self.w7_unsupported = true;
                Ok(ClassAtom::Char('[' as u32))
            }
            Some(c) => {
                self.pos += 1;
                Ok(ClassAtom::Char(c as u32))
            }
        }
    }

    fn parse_class_escape(&mut self, negated: bool) -> Result<ClassAtom, EcmaError> {
        self.pos += 1; // the backslash
        let e = match self.bump() {
            Some(c) => c,
            None => return Err(EcmaError::Malformed("unterminated escape".to_string())),
        };
        match e {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' => Ok(ClassAtom::Escape(class_escape(e, negated))),
            'b' => Err(EcmaError::Unsupported(
                "`\\b` inside a class is a backspace".to_string(),
            )),
            'B' => Err(EcmaError::Malformed(
                "`\\B` is not valid inside a class".to_string(),
            )),
            't' => Ok(ClassAtom::Char(0x9)),
            'n' => Ok(ClassAtom::Char(0xA)),
            'v' => Ok(ClassAtom::Char(0xB)),
            'f' => Ok(ClassAtom::Char(0xC)),
            'r' => Ok(ClassAtom::Char(0xD)),
            'x' => {
                let cp = self.read_hex(2)?;
                Ok(ClassAtom::Char(cp))
            }
            'u' => {
                let cp = self.parse_unicode_escape()?;
                Ok(ClassAtom::Char(cp))
            }
            'c' => Err(EcmaError::Unsupported(
                "`\\c` control escapes are not supported".to_string(),
            )),
            'p' | 'P' => Err(EcmaError::Unsupported(
                "`\\p{...}` property escapes are not supported".to_string(),
            )),
            '0' => Err(EcmaError::Unsupported("`\\0` is not supported".to_string())),
            '1'..='9' => Err(EcmaError::Malformed(
                "decimal escapes are not valid inside a class".to_string(),
            )),
            'k' => Err(EcmaError::Malformed(
                "`\\k` is not valid inside a class".to_string(),
            )),
            _ => {
                if is_syntax_char(e) || e == '-' {
                    Ok(ClassAtom::Char(e as u32))
                } else {
                    Err(EcmaError::Malformed(format!(
                        "identity escape `\\{e}` is not valid"
                    )))
                }
            }
        }
    }

    fn parse_quantifier(&mut self) -> Result<String, EcmaError> {
        match self.peek() {
            Some(c @ ('*' | '+' | '?')) => {
                self.pos += 1;
                let mut out = c.to_string();
                if self.eat('?') {
                    out.push('?');
                }
                Ok(out)
            }
            Some('{') => {
                self.pos += 1;
                let min = self.read_quantifier_number()?;
                let out = if self.eat('}') {
                    format!("{{{min}}}")
                } else if self.eat(',') {
                    if self.eat('}') {
                        format!("{{{min},}}")
                    } else {
                        let max = self.read_quantifier_number()?;
                        if !self.eat('}') {
                            return Err(EcmaError::Malformed(
                                "unterminated quantifier".to_string(),
                            ));
                        }
                        if min > max {
                            return Err(EcmaError::Malformed(
                                "quantifier bounds are out of order".to_string(),
                            ));
                        }
                        format!("{{{min},{max}}}")
                    }
                } else {
                    return Err(EcmaError::Malformed(
                        "a `{` must start a valid quantifier".to_string(),
                    ));
                };
                if self.eat('?') {
                    Ok(format!("{out}?"))
                } else {
                    Ok(out)
                }
            }
            _ => unreachable!("at_quantifier checked the lookahead"),
        }
    }

    fn read_quantifier_number(&mut self) -> Result<u64, EcmaError> {
        let mut value: u64 = 0;
        let mut any = false;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
            value = value.saturating_mul(10).saturating_add(u64::from(d));
            any = true;
            self.pos += 1;
        }
        if any {
            Ok(value)
        } else {
            Err(EcmaError::Malformed(
                "quantifier is missing a bound".to_string(),
            ))
        }
    }
}

/// Check a group name. Only ASCII names (`[A-Za-z_$][A-Za-z0-9_$]*`) are
/// accepted. ECMA's GroupName grammar uses Unicode ID_Start / ID_Continue,
/// which std does not expose, and `char::is_alphabetic` / `is_alphanumeric` are
/// no safe stand-in: they admit U+0345 (Other_Alphabetic, not ID_Start) and
/// '²' (No, not ID_Continue), which ECMA rejects. A non-ASCII name is therefore
/// `Unsupported` (it may be valid ECMA, but its validity is never guessed); an
/// ASCII name outside the grammar (empty, digit-leading) is `Malformed`.
fn validate_group_name(name: &str) -> Result<(), EcmaError> {
    if !name.is_ascii() {
        return Err(EcmaError::Unsupported(format!(
            "non-ASCII group name `{name}` is not supported"
        )));
    }
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => {
            return Err(EcmaError::Malformed(
                "group name must not be empty".to_string(),
            ))
        }
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return Err(EcmaError::Malformed(format!("invalid group name `{name}`")));
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '$') {
            return Err(EcmaError::Malformed(format!("invalid group name `{name}`")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, value: &str) -> bool {
        compile(pattern)
            .expect("pattern should translate and compile")
            .is_match(value)
    }

    fn is_unsupported(pattern: &str) -> bool {
        matches!(translate(pattern), Err(EcmaError::Unsupported(_)))
    }

    fn is_malformed(pattern: &str) -> bool {
        matches!(translate(pattern), Err(EcmaError::Malformed(_)))
    }

    #[test]
    fn translation_semantics() {
        // ASCII-only \d and \w.
        assert!(matches(r"^\d+$", "123"));
        assert!(!matches(r"^\d+$", "١٢٣"));
        assert!(matches(r"^\w+$", "a_1"));
        assert!(!matches(r"^\w+$", "é"));

        // Dot excludes ECMA's line terminators.
        assert!(matches(r"^.$", "a"));
        assert!(!matches(r"^.$", "\r"));
        assert!(!matches(r"^.$", "\u{2028}"));

        // \s / \S follow the ECMA set without U+0085 and U+FEFF.
        assert!(matches(r"^\s$", " "));
        assert!(!matches(r"^\s$", "\u{FEFF}"));
        assert!(!matches(r"^\s$", "\u{85}"));
        assert!(matches(r"^\S$", "a"));
        assert!(!matches(r"^\S$", "\u{FEFF}"));
        assert!(!matches(r"^\S$", "\u{85}"));

        // \D / \W keep the Unicode reading, which is the narrower one.
        assert!(matches(r"^\D$", "a"));
        assert!(!matches(r"^\D$", "١"));
        assert!(matches(r"^\W$", "!"));
        assert!(!matches(r"^\W$", "é"));

        // Boundaries hold only where both readings agree.
        assert!(!matches(r"^a\bé$", "aé"));
        assert!(!matches(r"^é\B!$", "é!"));
        assert!(matches(r"^\bab\b$", "ab"));

        // Negated and positive classes use the union/intersection forms.
        assert!(matches(r"^[^\w]$", "!"));
        assert!(!matches(r"^[^\w]$", "é"));
        assert!(matches(r"^[^\S]$", " "));
        assert!(!matches(r"^[^\S]$", "\u{FEFF}"));
        assert!(!matches(r"^[^\S]$", "\u{85}"));
        assert!(matches(r"^[^\D]$", "1"));
        assert!(!matches(r"^[^\D]$", "١"));
        assert!(matches(r"^[\D]$", "a"));
        assert!(!matches(r"^[\D]$", "١"));
        assert!(!matches(r"^[\s]$", "\u{FEFF}"));

        // Literal dashes and empty classes.
        assert!(matches(r"^[\d-]+$", "1-2"));
        assert!(!matches(r"^[]$", "x"));

        // Code point escapes, including a surrogate pair.
        assert!(matches(r"^\u{1F600}$", "😀"));
        assert!(matches(r"^\uD800\uDC00$", "\u{10000}"));

        // Named groups, quantifiers, laziness, and assertions in a group.
        assert!(matches(r"^(?<y>\d{4})-\d{2}$", "2024-01"));
        assert!(matches(r"^(?<a_B9>\w)$", "a"));
        assert!(matches(r"^a{2,3}?$", "aa"));
        assert!(matches(r"^(?:\b)*a$", "a"));
    }

    #[test]
    fn refuses_unsupported_constructs() {
        for pattern in [
            "(?=a)",
            "(?<!a)b",
            "(a)\\1",
            "(?<n>a)\\k<n>",
            "\\p{L}",
            "\\uD800",
            "\\uDC00\\uD800",
            "\\uD800\\uD801",
            "\\u{D800}",
            "(?<n>a)|(?<n>b)",
            "(?i:a)",
            "[a&&b]",
            "[a~~b]",
            "[[]",
            "[[:x]",
            "[^]",
            "\\cA",
            "\\0",
            "[\\b]",
        ] {
            assert!(
                is_unsupported(pattern),
                "expected Unsupported for {pattern:?}"
            );
        }

        // The nesting bound trips before the parser can exhaust the stack.
        let deep = format!("{}a{}", "(".repeat(300), ")".repeat(300));
        assert!(is_unsupported(&deep));
        let deeper = "(".repeat(100_000);
        assert!(is_unsupported(&deeper));
    }

    #[test]
    fn non_ascii_group_names_are_unsupported() {
        // std has no ID_Start/ID_Continue tables, and `is_alphabetic` /
        // `is_alphanumeric` admit names ECMA rejects (U+0345 is
        // Other_Alphabetic but not ID_Start; '²' is No, not ID_Continue).
        // Validity of a non-ASCII name is therefore never guessed.
        for pattern in ["(?<\u{0345}>a)", "(?<a²>a)", "(?<ünï>a)", "(?<Ⅻ>a)"] {
            assert!(
                is_unsupported(pattern),
                "expected Unsupported for {pattern:?}"
            );
        }
        assert!(matches(r"^(?<_aZ9>a)$", "a"));
    }

    #[test]
    fn refuses_malformed_constructs() {
        for pattern in [
            r"\b*",
            r"\b{2}",
            r"\8",
            r"\1",
            r"\k<n>",
            "(?<1a>x)",
            "(?<n>a)(?<n>b)",
            "(?P<n>a)",
            "(?i)a",
            r"\A",
            "a{,2}",
            r"\e",
            r"\-",
            r"[\B]",
            "[z-a]",
            "[a--b]",
            "[[:alpha:]]",
            "[[]]",
            "[[=x=]]",
            "\\uD800\\uDzz",
            "\\uD800\\uDC0",
            "(a",
            "a)",
            "{",
            "}",
            "]",
        ] {
            assert!(is_malformed(pattern), "expected Malformed for {pattern:?}");
        }
    }
}
