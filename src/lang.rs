//! Which language a message comes out in.
//!
//! It is a parameter and not a global for one reason: the CLI and the MCP
//! server run in the same process and want different answers. A person
//! standing in a terminal gets their own language; the model on the other
//! end of `mem serve` gets English, because the right language there is the
//! one the conversation is in, which this program cannot know, and English
//! is the substrate the model translates from.
//!
//! Nothing here chooses the language — there is no locale detection, no
//! environment, no config. Choosing is a later job; this is only the knob it
//! will turn, introduced early because the functions the two surfaces share
//! are the ones that need it.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

impl Lang {
    /// Picks between two literals. It reads better at the call site than a
    /// `match` when the only thing that changes is the string:
    ///
    /// ```ignore
    /// bail!("{}", lang.pick("window cannot be empty", "la ventana no puede estar vacía"));
    /// ```
    pub fn pick<'a>(self, en: &'a str, es: &'a str) -> &'a str {
        match self {
            Lang::En => en,
            Lang::Es => es,
        }
    }
}
