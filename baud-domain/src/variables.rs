//! **The variables an interpolated string can name**, as iTerm2's
//! Variables reference lists them (`[[source: iterm2-doc-variables]]`):
//! the session's, a session's user-defined ones, `user.<name>`, and the
//! application's, `iterm2.<name>`, each with what baud answers for it
//! ([`Answer`]). One table, so the core's count of what baud cannot answer
//! and the embedder's answers read the same list.

/// **What baud has for a variable.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    /// Its value, which the embedder gives.
    Answered,
    /// Never set, truthfully: iTerm2 sets it only for a feature baud does
    /// not have -- a trigger, tmux integration -- and without that feature
    /// it is undefined there too.
    Unset,
    /// iTerm2 has a value baud cannot give, or the reference names no such
    /// variable: drawn as an undefined one is, and counted.
    Unknown,
}

/// Declares [`SessionVariable`]: each variable's name as the reference
/// writes it, and what baud has for it.
macro_rules! session_variables {
    ($($v:ident $name:literal $answer:ident,)*) => {
        /// **A variable "defined in the context of a session"**, every one
        /// the reference lists.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum SessionVariable {
            $($v,)*
        }

        impl SessionVariable {
            /// Every one, in the reference's order.
            pub const ALL: &'static [SessionVariable] = &[$(SessionVariable::$v,)*];

            /// Its name, as a path names it.
            pub fn name(self) -> &'static str {
                match self {
                    $(SessionVariable::$v => $name,)*
                }
            }

            /// The variable `name` is, if the reference lists it.
            pub fn of(name: &str) -> Option<SessionVariable> {
                match name {
                    $($name => Some(SessionVariable::$v),)*
                    _ => None,
                }
            }

            /// What baud has for it.
            pub fn answer(self) -> Answer {
                match self {
                    $(SessionVariable::$v => Answer::$answer,)*
                }
            }
        }
    };
}

session_variables! {
    // Session Name. baud has no profile, whose name `autoNameFormat` starts
    // as; `name` and `presentationName` are the window's title as shown,
    // the one place baud shows a session's name.
    AutoNameFormat "autoNameFormat" Unknown,
    AutoName "autoName" Unknown,
    Name "name" Answered,
    PresentationName "presentationName" Answered,
    TerminalIconName "terminalIconName" Answered,
    TerminalWindowName "terminalWindowName" Answered,
    TriggerName "triggerName" Unset,
    // Terminal.
    Columns "columns" Answered,
    CommandLine "commandLine" Unknown,
    JobName "jobName" Answered,
    DeepestJob "deepestJob" Unknown,
    JobPid "jobPid" Answered,
    MouseReportingMode "mouseReportingMode" Answered,
    ParentSession "parentSession" Unknown,
    Pid "pid" Answered,
    ProcessTitle "processTitle" Unknown,
    Rows "rows" Answered,
    Selection "selection" Answered,
    SelectionLength "selectionLength" Answered,
    Termid "termid" Unknown,
    Tty "tty" Answered,
    // `uname`, `shell` and `homeDirectory` are this machine's, as iTerm2's
    // are without ssh integration, which baud has not
    // (`updateVariablesFromConductor`, `[[source:
    // iterm2-964a75d-ptysession]]`).
    Uname "uname" Answered,
    Shell "shell" Answered,
    SshIntegrationLevel "sshIntegrationLevel" Answered,
    HomeDirectory "homeDirectory" Answered,
    ApplicationKeypad "applicationKeypad" Answered,
    MouseInfo "mouseInfo" Unknown,
    BellCount "bellCount" Answered,
    // Shell Integration.
    Hostname "hostname" Answered,
    LastCommand "lastCommand" Unknown,
    Path "path" Answered,
    Username "username" Answered,
    // Logging. iTerm2 sets `autoLogId`, a random number its automatic logs
    // are named by, and `creationTimeString` when every session starts,
    // logging or not (`initSynthetic:`, `[[source:
    // iterm2-964a75d-ptysession]]`): baud, which names no log by such a
    // number, gives the time and not the number. xterm's logging, which
    // baud has, names its file where iTerm2's `logFilename` would.
    AutoLogId "autoLogId" Unknown,
    CreationTimeString "creationTimeString" Answered,
    LogFilename "logFilename" Unknown,
    // tmux Integration, which baud has not.
    TmuxClientName "tmuxClientName" Unset,
    TmuxPaneTitle "tmuxPaneTitle" Unset,
    TmuxRole "tmuxRole" Unset,
    TmuxStatusLeft "tmuxStatusLeft" Unset,
    TmuxStatusRight "tmuxStatusRight" Unset,
    TmuxWindowPane "tmuxWindowPane" Unset,
    TmuxWindowTitle "tmuxWindowTitle" Unset,
    TmuxWindowPaneIndex "tmuxWindowPaneIndex" Unset,
    // Other. `badge` in the badge's own format is a cycle, which iTerm2
    // answers by drawing "[Cycle detected]".
    Badge "badge" Answered,
    Id "id" Unknown,
    ProfileName "profileName" Unknown,
}

/// **A variable of the "Global Context"**, `iterm2.<name>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlobalVariable {
    /// "The path to iTerm2.app", which baud is not.
    AppBundlePath,
    EffectiveTheme,
    /// "The best guess of what localhost's hostname is".
    LocalhostName,
    /// "The process ID of the iTerm2 app": the terminal's own.
    Pid,
}

impl GlobalVariable {
    pub fn of(name: &str) -> Option<GlobalVariable> {
        Some(match name {
            "appBundlePath" => GlobalVariable::AppBundlePath,
            "effectiveTheme" => GlobalVariable::EffectiveTheme,
            "localhostName" => GlobalVariable::LocalhostName,
            "pid" => GlobalVariable::Pid,
            _ => return None,
        })
    }

    pub fn answer(self) -> Answer {
        match self {
            GlobalVariable::LocalhostName | GlobalVariable::Pid => Answer::Answered,
            GlobalVariable::AppBundlePath | GlobalVariable::EffectiveTheme => Answer::Unknown,
        }
    }
}

/// **What a path names**, read as a session's context reads it: `user.`
/// before a user-defined variable, `iterm2.` before the application's, a
/// leading `session.` the session itself (iTerm2's alias for its legacy
/// paths, `[[source: iterm2-964a75d-ptysession]]`), and the rest a
/// session variable. A tab's or a window's context, which a session
/// reaches through `tab`, is none baud answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named<'a> {
    Session(SessionVariable),
    /// `user.<name>`: the name after `user.`.
    User(&'a str),
    Global(GlobalVariable),
    /// A path the reference does not list.
    Other,
}

impl<'a> Named<'a> {
    pub fn of(path: &'a str) -> Named<'a> {
        let mut p = path;
        while let Some(rest) = p.strip_prefix("session.") {
            p = rest;
        }
        if let Some(name) = p.strip_prefix("user.") {
            return match name.is_empty() {
                true => Named::Other,
                false => Named::User(name),
            };
        }
        if let Some(name) = p.strip_prefix("iterm2.") {
            return GlobalVariable::of(name).map_or(Named::Other, Named::Global);
        }
        SessionVariable::of(p).map_or(Named::Other, Named::Session)
    }

    /// What baud has for it: a user-defined variable is always answered,
    /// its value or undefined.
    pub fn answer(self) -> Answer {
        match self {
            Named::Session(v) => v.answer(),
            Named::User(_) => Answer::Answered,
            Named::Global(g) => g.answer(),
            Named::Other => Answer::Unknown,
        }
    }
}
