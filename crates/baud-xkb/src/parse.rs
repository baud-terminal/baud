//! **The keymap text, read into its four sections.** What a compositor
//! sends in `wl_keyboard.keymap` is one `xkb_keymap { ... }` block with
//! everything inlined -- no `include`, no rules -- and this reads exactly
//! that shape (`[[source: xkb-keymap-text-format]]`). Names are kept as
//! names here: which real modifier a virtual one encodes, which type a
//! key's group uses and which action a level performs are all resolved by
//! the compile step, once every section has been read.

use crate::keysym::Keysym;
use crate::lex::{Lexer, Tok};
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug)]
pub struct Error {
    pub line: usize,
    pub what: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.what)
    }
}

/// A modifier expression as written: names joined by `+`, or `none`,
/// `all`, a number, or `modMapMods`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModExpr {
    pub names: Vec<String>,
    pub all: bool,
    pub mask: u32,
    pub mod_map_mods: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchOp {
    AnyOfOrNone,
    AnyOf,
    NoneOf,
    AllOf,
    Exactly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSpec {
    /// One-based.
    Absolute(u32),
    Relative(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SetMods {
        mods: ModExpr,
        clear_locks: bool,
    },
    LatchMods {
        mods: ModExpr,
        clear_locks: bool,
        latch_to_lock: bool,
    },
    LockMods {
        mods: ModExpr,
    },
    SetGroup {
        group: GroupSpec,
        clear_locks: bool,
    },
    LatchGroup {
        group: GroupSpec,
        latch_to_lock: bool,
    },
    LockGroup {
        group: GroupSpec,
    },
    /// Anything else the compat section names -- pointer, control and
    /// legacy actions -- which a terminal does not act on.
    Other,
}

#[derive(Clone, Debug)]
pub struct TypeDef {
    pub name: String,
    pub mods: ModExpr,
    /// `(modifiers, level)`, level zero-based.
    pub map: Vec<(ModExpr, usize)>,
    pub preserve: Vec<(ModExpr, ModExpr)>,
    pub levels: usize,
}

#[derive(Clone, Debug)]
pub struct Interpret {
    pub sym: Option<Keysym>,
    pub op: MatchOp,
    pub mods: ModExpr,
    pub level1: bool,
    pub vmod: Option<String>,
    pub repeat: Option<bool>,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Default)]
pub struct GroupDef {
    pub type_name: Option<String>,
    pub syms: Vec<Vec<Keysym>>,
    pub actions: Vec<Vec<Action>>,
}

#[derive(Clone, Debug, Default)]
pub struct KeyDef {
    pub name: String,
    pub groups: Vec<GroupDef>,
    /// `type=` without a group index applies to every group.
    pub type_all: Option<String>,
    pub repeat: Option<bool>,
    pub vmods: Option<ModExpr>,
}

#[derive(Clone, Debug)]
pub enum KeyRef {
    Name(String),
    Sym(Keysym),
}

#[derive(Debug, Default)]
pub struct Source {
    pub key_names: Vec<(String, u32)>,
    pub aliases: Vec<(String, String)>,
    pub min_keycode: u32,
    pub max_keycode: u32,
    pub vmods: Vec<(String, Option<u32>)>,
    pub types: Vec<TypeDef>,
    pub interprets: Vec<Interpret>,
    pub interpret_repeat_default: bool,
    pub interpret_level1_default: bool,
    pub keys: Vec<KeyDef>,
    /// `(real modifier name, keys)`.
    pub modmap: Vec<(String, Vec<KeyRef>)>,
    pub group_names: Vec<String>,
}

struct P<'a> {
    lx: Lexer<'a>,
}

type R<T> = Result<T, Error>;

impl<'a> P<'a> {
    fn err<T>(&self, what: &'static str) -> R<T> {
        Err(Error {
            line: self.lx.line,
            what,
        })
    }

    fn expect(&mut self, want: Tok<'_>) -> R<()> {
        let t = self.lx.take();
        match t == want {
            true => Ok(()),
            false => self.err("unexpected token"),
        }
    }

    fn punct(&mut self, c: char) -> R<()> {
        self.expect(Tok::Punct(c))
    }

    fn eat(&mut self, want: Tok<'_>) -> bool {
        if self.lx.peek() == want {
            self.lx.take();
            return true;
        }
        false
    }

    fn ident(&mut self) -> R<&'a str> {
        match self.lx.take() {
            Tok::Ident(s) => Ok(s),
            _ => self.err("expected a name"),
        }
    }

    fn string(&mut self) -> R<&'a str> {
        match self.lx.take() {
            Tok::Str(s) => Ok(s),
            _ => self.err("expected a string"),
        }
    }

    fn int(&mut self) -> R<u32> {
        match self.lx.take() {
            Tok::Int(v) => Ok(v),
            _ => self.err("expected a number"),
        }
    }

    /// Skip a balanced `{ ... }` block whose contents are not read.
    fn skip_block(&mut self) -> R<()> {
        self.punct('{')?;
        let mut depth = 1;
        while depth > 0 {
            match self.lx.take() {
                Tok::Punct('{') => depth += 1,
                Tok::Punct('}') => depth -= 1,
                Tok::End => return self.err("unterminated block"),
                _ => {}
            }
        }
        Ok(())
    }

    /// Skip to the end of the statement.
    fn skip_statement(&mut self) -> R<()> {
        loop {
            match self.lx.take() {
                Tok::Punct(';') => return Ok(()),
                Tok::Punct('{') => {
                    let mut depth = 1;
                    while depth > 0 {
                        match self.lx.take() {
                            Tok::Punct('{') => depth += 1,
                            Tok::Punct('}') => depth -= 1,
                            Tok::End => return self.err("unterminated block"),
                            _ => {}
                        }
                    }
                }
                Tok::End => return self.err("unterminated statement"),
                _ => {}
            }
        }
    }

    /// `A+B+C`, `none`, `all`, a number, or `modMapMods`.
    fn mod_expr(&mut self) -> R<ModExpr> {
        let mut e = ModExpr::default();
        loop {
            match self.lx.take() {
                Tok::Ident(s) if s.eq_ignore_ascii_case("none") => {}
                Tok::Ident(s) if s.eq_ignore_ascii_case("all") => e.all = true,
                Tok::Ident(s) if s.eq_ignore_ascii_case("modmapmods") => e.mod_map_mods = true,
                Tok::Ident(s) => e.names.push(s.to_string()),
                Tok::Int(v) => e.mask |= v,
                _ => return self.err("expected a modifier"),
            }
            if !self.eat(Tok::Punct('+')) {
                return Ok(e);
            }
        }
    }

    fn boolean(&mut self) -> R<bool> {
        match self.lx.take() {
            Tok::Ident(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" => Ok(true),
                "false" | "no" | "off" => Ok(false),
                _ => self.err("expected a boolean"),
            },
            Tok::Int(v) => Ok(v != 0),
            _ => self.err("expected a boolean"),
        }
    }

    /// `Level3`, `Group2`, or a number: one-based, returned zero-based.
    fn index(&mut self) -> R<usize> {
        match self.lx.take() {
            Tok::Int(v) => Ok(usize::try_from(v.max(1)).map_err(|_| Error {
                line: 0,
                what: "index",
            })? - 1),
            Tok::Ident(s) => {
                let digits = s.trim_start_matches("Level").trim_start_matches("Group");
                match digits.parse::<usize>() {
                    Ok(n) if n >= 1 => Ok(n - 1),
                    _ => self.err("expected a level or group index"),
                }
            }
            _ => self.err("expected an index"),
        }
    }

    fn group_spec(&mut self) -> R<GroupSpec> {
        match self.lx.take() {
            Tok::Punct('+') => Ok(GroupSpec::Relative(i32::try_from(self.int()?).unwrap_or(0))),
            Tok::Punct('-') => Ok(GroupSpec::Relative(
                -i32::try_from(self.int()?).unwrap_or(0),
            )),
            Tok::Int(v) => Ok(GroupSpec::Absolute(v)),
            Tok::Ident(s) => match s.trim_start_matches("Group").parse::<u32>() {
                Ok(n) => Ok(GroupSpec::Absolute(n)),
                Err(_) => self.err("expected a group"),
            },
            _ => self.err("expected a group"),
        }
    }

    /// `Name(param=value, flag, !flag, ...)`.
    fn action(&mut self) -> R<Action> {
        let name = self.ident()?.to_ascii_lowercase();
        self.punct('(')?;
        let mut mods = ModExpr::default();
        let mut group = GroupSpec::Relative(0);
        let (mut clear_locks, mut latch_to_lock) = (false, false);
        if !self.eat(Tok::Punct(')')) {
            loop {
                let negated = self.eat(Tok::Punct('!'));
                let key = self.ident()?.to_ascii_lowercase();
                // `data[0]=0x50`: an indexed parameter of a private action.
                if self.eat(Tok::Punct('[')) {
                    while !matches!(self.lx.take(), Tok::Punct(']') | Tok::End) {}
                }
                let has_value = self.eat(Tok::Punct('='));
                match key.as_str() {
                    "modifiers" | "mods" if has_value => mods = self.mod_expr()?,
                    "group" if has_value => group = self.group_spec()?,
                    "clearlocks" => clear_locks = !negated && (!has_value || self.boolean()?),
                    "latchtolock" => latch_to_lock = !negated && (!has_value || self.boolean()?),
                    _ => {
                        if has_value {
                            // A value of any shape: skip to the separator.
                            loop {
                                match self.lx.peek() {
                                    Tok::Punct(',') | Tok::Punct(')') => break,
                                    Tok::End => return self.err("unterminated action"),
                                    _ => {
                                        self.lx.take();
                                    }
                                }
                            }
                        }
                    }
                }
                if self.eat(Tok::Punct(')')) {
                    break;
                }
                self.punct(',')?;
            }
        }
        Ok(match name.as_str() {
            "setmods" => Action::SetMods { mods, clear_locks },
            "latchmods" => Action::LatchMods {
                mods,
                clear_locks,
                latch_to_lock,
            },
            "lockmods" => Action::LockMods { mods },
            "setgroup" => Action::SetGroup { group, clear_locks },
            "latchgroup" => Action::LatchGroup {
                group,
                latch_to_lock,
            },
            "lockgroup" => Action::LockGroup { group },
            _ => Action::Other,
        })
    }

    /// One action or `{ a, b }`.
    fn actions(&mut self) -> R<Vec<Action>> {
        if self.eat(Tok::Punct('{')) {
            let mut v = Vec::new();
            if !self.eat(Tok::Punct('}')) {
                loop {
                    v.push(self.action()?);
                    if self.eat(Tok::Punct('}')) {
                        break;
                    }
                    self.punct(',')?;
                }
            }
            return Ok(v);
        }
        Ok(alloc::vec![self.action()?])
    }

    /// `[ a, A ]` -- each level one keysym or `{ a, b }`.
    fn keysyms(&mut self) -> R<Vec<Vec<Keysym>>> {
        self.punct('[')?;
        let mut levels = Vec::new();
        if self.eat(Tok::Punct(']')) {
            return Ok(levels);
        }
        loop {
            let mut level = Vec::new();
            if self.eat(Tok::Punct('{')) {
                if !self.eat(Tok::Punct('}')) {
                    loop {
                        level.push(self.keysym()?);
                        if self.eat(Tok::Punct('}')) {
                            break;
                        }
                        self.punct(',')?;
                    }
                }
            } else {
                level.push(self.keysym()?);
            }
            level.retain(|k| *k != Keysym::NO_SYMBOL);
            levels.push(level);
            if self.eat(Tok::Punct(']')) {
                return Ok(levels);
            }
            self.punct(',')?;
        }
    }

    fn keysym(&mut self) -> R<Keysym> {
        match self.lx.take() {
            Tok::Ident(s) => match Keysym::named(s) {
                Some(k) => Ok(k),
                None if s.eq_ignore_ascii_case("nosymbol")
                    || s.eq_ignore_ascii_case("voidsymbol") =>
                {
                    Ok(Keysym::NO_SYMBOL)
                }
                None => Ok(Keysym::NO_SYMBOL),
            },
            // A bare digit is the digit's keysym -- `[ 0, parenright ]` --
            // which no numeric keysym below ten could be.
            Tok::Int(v) if v < 10 => Ok(Keysym(0x30 + v)),
            Tok::Int(v) => Ok(Keysym(v)),
            _ => self.err("expected a keysym"),
        }
    }

    /// `[ { a, b }, c ]` of actions per level.
    fn action_list(&mut self) -> R<Vec<Vec<Action>>> {
        self.punct('[')?;
        let mut levels = Vec::new();
        if self.eat(Tok::Punct(']')) {
            return Ok(levels);
        }
        loop {
            levels.push(self.actions()?);
            if self.eat(Tok::Punct(']')) {
                return Ok(levels);
            }
            self.punct(',')?;
        }
    }

    fn keycodes(&mut self, src: &mut Source) -> R<()> {
        // The optional name string was taken by the caller.
        self.punct('{')?;
        while !self.eat(Tok::Punct('}')) {
            match self.lx.take() {
                Tok::KeyName(n) => {
                    self.punct('=')?;
                    let v = self.int()?;
                    src.key_names.push((n.to_string(), v));
                    self.punct(';')?;
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("minimum") => {
                    self.punct('=')?;
                    src.min_keycode = self.int()?;
                    self.punct(';')?;
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("maximum") => {
                    self.punct('=')?;
                    src.max_keycode = self.int()?;
                    self.punct(';')?;
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("alias") => {
                    let a = match self.lx.take() {
                        Tok::KeyName(n) => n.to_string(),
                        _ => return self.err("alias name"),
                    };
                    self.punct('=')?;
                    let b = match self.lx.take() {
                        Tok::KeyName(n) => n.to_string(),
                        _ => return self.err("alias target"),
                    };
                    src.aliases.push((a, b));
                    self.punct(';')?;
                }
                // `indicator 1 = "Caps Lock";` and anything else.
                Tok::End => return self.err("unterminated keycodes"),
                _ => self.skip_statement()?,
            }
        }
        self.punct(';')?;
        Ok(())
    }

    fn vmod_decl(&mut self, src: &mut Source) -> R<()> {
        loop {
            let name = self.ident()?.to_string();
            let mask = match self.eat(Tok::Punct('=')) {
                true => Some(self.mod_expr()?.mask),
                false => None,
            };
            if !src.vmods.iter().any(|(n, _)| *n == name) {
                src.vmods.push((name, mask));
            } else if let (Some(m), Some(slot)) =
                (mask, src.vmods.iter_mut().find(|(n, _)| *n == name))
            {
                slot.1 = Some(m);
            }
            if !self.eat(Tok::Punct(',')) {
                break;
            }
        }
        self.punct(';')
    }

    fn types(&mut self, src: &mut Source) -> R<()> {
        self.punct('{')?;
        while !self.eat(Tok::Punct('}')) {
            match self.lx.take() {
                Tok::Ident(s) if s.eq_ignore_ascii_case("virtual_modifiers") => {
                    self.vmod_decl(src)?
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("type") => {
                    let name = self.string()?.to_string();
                    self.punct('{')?;
                    let mut t = TypeDef {
                        name,
                        mods: ModExpr::default(),
                        map: Vec::new(),
                        preserve: Vec::new(),
                        levels: 1,
                    };
                    while !self.eat(Tok::Punct('}')) {
                        match self.lx.take() {
                            Tok::Ident(f) if f.eq_ignore_ascii_case("modifiers") => {
                                self.punct('=')?;
                                t.mods = self.mod_expr()?;
                                self.punct(';')?;
                            }
                            Tok::Ident(f) if f.eq_ignore_ascii_case("map") => {
                                self.punct('[')?;
                                let m = self.mod_expr()?;
                                self.punct(']')?;
                                self.punct('=')?;
                                let l = self.index()?;
                                t.levels = t.levels.max(l + 1);
                                t.map.push((m, l));
                                self.punct(';')?;
                            }
                            Tok::Ident(f) if f.eq_ignore_ascii_case("preserve") => {
                                self.punct('[')?;
                                let m = self.mod_expr()?;
                                self.punct(']')?;
                                self.punct('=')?;
                                let p = self.mod_expr()?;
                                t.preserve.push((m, p));
                                self.punct(';')?;
                            }
                            Tok::Ident(f) if f.eq_ignore_ascii_case("level_name") => {
                                self.punct('[')?;
                                let l = self.index()?;
                                t.levels = t.levels.max(l + 1);
                                self.punct(']')?;
                                self.punct('=')?;
                                self.string()?;
                                self.punct(';')?;
                            }
                            Tok::End => return self.err("unterminated type"),
                            _ => self.skip_statement()?,
                        }
                    }
                    self.punct(';')?;
                    src.types.push(t);
                }
                Tok::End => return self.err("unterminated types"),
                _ => self.skip_statement()?,
            }
        }
        self.punct(';')
    }

    fn compat(&mut self, src: &mut Source) -> R<()> {
        self.punct('{')?;
        while !self.eat(Tok::Punct('}')) {
            match self.lx.take() {
                Tok::Ident(s) if s.eq_ignore_ascii_case("virtual_modifiers") => {
                    self.vmod_decl(src)?
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("interpret") => {
                    if self.eat(Tok::Punct('.')) {
                        let f = self.ident()?.to_ascii_lowercase();
                        self.punct('=')?;
                        match f.as_str() {
                            "usemodmapmods" => {
                                let v = self.ident()?;
                                src.interpret_level1_default = v.eq_ignore_ascii_case("level1");
                            }
                            "repeat" => src.interpret_repeat_default = self.boolean()?,
                            _ => {
                                self.skip_statement()?;
                                continue;
                            }
                        }
                        self.punct(';')?;
                        continue;
                    }
                    let sym = match self.lx.take() {
                        Tok::Ident(s) if s.eq_ignore_ascii_case("any") => None,
                        Tok::Ident(s) => match Keysym::named(s) {
                            Some(k) if k != Keysym::NO_SYMBOL => Some(k),
                            _ => None,
                        },
                        Tok::Int(v) => Some(Keysym(v)),
                        _ => return self.err("interpret keysym"),
                    };
                    let (mut op, mut mods) = (
                        MatchOp::AnyOfOrNone,
                        ModExpr {
                            all: true,
                            ..ModExpr::default()
                        },
                    );
                    if self.eat(Tok::Punct('+')) {
                        let o = self.ident()?.to_ascii_lowercase();
                        op = match o.as_str() {
                            "anyofornone" => MatchOp::AnyOfOrNone,
                            "anyof" => MatchOp::AnyOf,
                            "noneof" => MatchOp::NoneOf,
                            "allof" => MatchOp::AllOf,
                            "exactly" => MatchOp::Exactly,
                            "any" => MatchOp::AnyOf,
                            _ => MatchOp::Exactly,
                        };
                        if self.eat(Tok::Punct('(')) {
                            mods = self.mod_expr()?;
                            self.punct(')')?;
                        } else if o.as_str() == "any" {
                            mods = ModExpr {
                                all: true,
                                ..ModExpr::default()
                            };
                        } else {
                            // `interpret X+Shift` -- a bare mask means Exactly.
                            op = MatchOp::Exactly;
                            mods = ModExpr {
                                names: alloc::vec![o],
                                ..ModExpr::default()
                            };
                            while self.eat(Tok::Punct('+')) {
                                mods.names.push(self.ident()?.to_string());
                            }
                        }
                    }
                    let mut i = Interpret {
                        sym,
                        op,
                        mods,
                        level1: src.interpret_level1_default,
                        vmod: None,
                        repeat: None,
                        actions: Vec::new(),
                    };
                    self.punct('{')?;
                    while !self.eat(Tok::Punct('}')) {
                        match self.lx.take() {
                            Tok::Ident(f) if f.eq_ignore_ascii_case("usemodmapmods") => {
                                self.punct('=')?;
                                i.level1 = self.ident()?.eq_ignore_ascii_case("level1");
                                self.punct(';')?;
                            }
                            Tok::Ident(f) if f.eq_ignore_ascii_case("virtualmodifier") => {
                                self.punct('=')?;
                                i.vmod = Some(self.ident()?.to_string());
                                self.punct(';')?;
                            }
                            Tok::Ident(f) if f.eq_ignore_ascii_case("repeat") => {
                                self.punct('=')?;
                                i.repeat = Some(self.boolean()?);
                                self.punct(';')?;
                            }
                            Tok::Ident(f) if f.eq_ignore_ascii_case("action") => {
                                self.punct('=')?;
                                i.actions = self.actions()?;
                                self.punct(';')?;
                            }
                            Tok::End => return self.err("unterminated interpret"),
                            _ => self.skip_statement()?,
                        }
                    }
                    self.punct(';')?;
                    src.interprets.push(i);
                }
                Tok::End => return self.err("unterminated compat"),
                _ => self.skip_statement()?,
            }
        }
        self.punct(';')
    }

    fn key(&mut self, src: &mut Source) -> R<()> {
        let name = match self.lx.take() {
            Tok::KeyName(n) => n.to_string(),
            _ => return self.err("key name"),
        };
        let mut k = KeyDef {
            name,
            ..KeyDef::default()
        };
        self.punct('{')?;
        while !self.eat(Tok::Punct('}')) {
            match self.lx.peek() {
                Tok::Punct('[') => {
                    let syms = self.keysyms()?;
                    group_mut(&mut k, 0).syms = syms;
                }
                Tok::Ident(_) => {
                    let f = self.ident()?.to_ascii_lowercase();
                    let g = match self.eat(Tok::Punct('[')) {
                        true => {
                            let g = self.index()?;
                            self.punct(']')?;
                            Some(g)
                        }
                        false => None,
                    };
                    self.punct('=')?;
                    match f.as_str() {
                        "type" => {
                            let t = self.string()?.to_string();
                            match g {
                                Some(g) => group_mut(&mut k, g).type_name = Some(t),
                                None => k.type_all = Some(t),
                            }
                        }
                        "symbols" => {
                            let s = self.keysyms()?;
                            group_mut(&mut k, g.unwrap_or(0)).syms = s;
                        }
                        "actions" => {
                            let a = self.action_list()?;
                            group_mut(&mut k, g.unwrap_or(0)).actions = a;
                        }
                        "repeat" | "repeats" => k.repeat = Some(self.boolean()?),
                        "virtualmodifiers" | "vmods" | "virtualmods" => {
                            k.vmods = Some(self.mod_expr()?)
                        }
                        _ => {
                            // groupsWrap, groupsClamp, groupsRedirect, locks ...
                            while !matches!(
                                self.lx.peek(),
                                Tok::Punct(',') | Tok::Punct('}') | Tok::End
                            ) {
                                self.lx.take();
                            }
                        }
                    }
                }
                Tok::End => return self.err("unterminated key"),
                _ => return self.err("unexpected in key"),
            }
            if !self.eat(Tok::Punct(',')) {
                self.punct('}')?;
                break;
            }
        }
        self.punct(';')?;
        src.keys.push(k);
        Ok(())
    }

    fn symbols(&mut self, src: &mut Source) -> R<()> {
        self.punct('{')?;
        while !self.eat(Tok::Punct('}')) {
            match self.lx.take() {
                Tok::Ident(s) if s.eq_ignore_ascii_case("virtual_modifiers") => {
                    self.vmod_decl(src)?
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("key") => {
                    if self.eat(Tok::Punct('.')) {
                        // key.type = "..." and friends: defaults this reader
                        // does not model (the emitted keymap names every type).
                        self.skip_statement()?;
                        continue;
                    }
                    self.key(src)?;
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("name") => {
                    self.punct('[')?;
                    let g = self.index()?;
                    self.punct(']')?;
                    self.punct('=')?;
                    let n = self.string()?.to_string();
                    if src.group_names.len() <= g {
                        src.group_names.resize(g + 1, String::new());
                    }
                    src.group_names[g] = n;
                    self.punct(';')?;
                }
                Tok::Ident(s) if s.eq_ignore_ascii_case("modifier_map") => {
                    let m = self.ident()?.to_string();
                    self.punct('{')?;
                    let mut keys = Vec::new();
                    if !self.eat(Tok::Punct('}')) {
                        loop {
                            keys.push(match self.lx.take() {
                                Tok::KeyName(n) => KeyRef::Name(n.to_string()),
                                Tok::Ident(n) => {
                                    KeyRef::Sym(Keysym::named(n).unwrap_or(Keysym::NO_SYMBOL))
                                }
                                _ => return self.err("modifier_map entry"),
                            });
                            if self.eat(Tok::Punct('}')) {
                                break;
                            }
                            self.punct(',')?;
                        }
                    }
                    self.punct(';')?;
                    src.modmap.push((m, keys));
                }
                Tok::End => return self.err("unterminated symbols"),
                _ => self.skip_statement()?,
            }
        }
        self.punct(';')
    }

    fn keymap(&mut self) -> R<Source> {
        let mut src = Source::default();
        // Optional flags before `xkb_keymap`.
        loop {
            match self.lx.take() {
                Tok::Ident(s) if s.eq_ignore_ascii_case("xkb_keymap") => break,
                Tok::Ident(_) => {}
                _ => return self.err("expected xkb_keymap"),
            }
        }
        if let Tok::Str(_) = self.lx.peek() {
            self.lx.take();
        }
        self.punct('{')?;
        while !self.eat(Tok::Punct('}')) {
            // Section flags (`partial`, `default`, ...) then the section.
            let section = loop {
                match self.lx.take() {
                    Tok::Ident(s) if s.starts_with("xkb_") => break s.to_ascii_lowercase(),
                    Tok::Ident(_) => {}
                    Tok::End => return self.err("unterminated keymap"),
                    _ => return self.err("expected a section"),
                }
            };
            if let Tok::Str(_) = self.lx.peek() {
                self.lx.take();
            }
            match section.as_str() {
                "xkb_keycodes" => self.keycodes(&mut src)?,
                "xkb_types" => self.types(&mut src)?,
                "xkb_compatibility" | "xkb_compat" => self.compat(&mut src)?,
                "xkb_symbols" => self.symbols(&mut src)?,
                _ => {
                    self.skip_block()?;
                    self.punct(';')?;
                }
            }
        }
        let _ = self.eat(Tok::Punct(';'));
        Ok(src)
    }
}

fn group_mut(k: &mut KeyDef, g: usize) -> &mut GroupDef {
    if k.groups.len() <= g {
        k.groups.resize_with(g + 1, GroupDef::default);
    }
    &mut k.groups[g]
}

/// Read a keymap.
pub fn parse(text: &str) -> Result<Source, Error> {
    P {
        lx: Lexer::new(text),
    }
    .keymap()
}
