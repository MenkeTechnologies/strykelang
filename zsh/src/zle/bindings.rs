//! Key binding types and parsing

/// A key sequence (one or more keys)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence(pub Vec<KeyBinding>);

impl KeySequence {
    pub fn parse(s: &str) -> Self {
        // Parse key sequence string into individual key bindings
        let mut bindings = Vec::new();
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            let binding = match c {
                '^' => {
                    if let Some(&next) = chars.peek() {
                        chars.next();
                        KeyBinding::Control(next)
                    } else {
                        KeyBinding::Char(c)
                    }
                }
                '\\' => {
                    if let Some(&next) = chars.peek() {
                        chars.next();
                        match next {
                            'e' | 'E' => KeyBinding::Escape,
                            'C' => {
                                if chars.peek() == Some(&'-') {
                                    chars.next();
                                    if let Some(ctrl_char) = chars.next() {
                                        KeyBinding::Control(ctrl_char)
                                    } else {
                                        KeyBinding::Char('C')
                                    }
                                } else {
                                    KeyBinding::Char('C')
                                }
                            }
                            'M' => {
                                if chars.peek() == Some(&'-') {
                                    chars.next();
                                    if let Some(meta_char) = chars.next() {
                                        KeyBinding::Meta(meta_char)
                                    } else {
                                        KeyBinding::Char('M')
                                    }
                                } else {
                                    KeyBinding::Char('M')
                                }
                            }
                            'n' => KeyBinding::Char('\n'),
                            't' => KeyBinding::Char('\t'),
                            'r' => KeyBinding::Char('\r'),
                            '\\' => KeyBinding::Char('\\'),
                            _ => KeyBinding::Char(next),
                        }
                    } else {
                        KeyBinding::Char(c)
                    }
                }
                _ => KeyBinding::Char(c),
            };
            bindings.push(binding);
        }

        KeySequence(bindings)
    }

    pub fn to_string(&self) -> String {
        self.0.iter().map(|b| b.to_display_string()).collect()
    }
}

/// A single key binding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBinding {
    /// Plain character
    Char(char),
    /// Control + character
    Control(char),
    /// Meta/Alt + character
    Meta(char),
    /// Escape key
    Escape,
    /// Function key
    Function(u8),
    /// Arrow keys
    Up,
    Down,
    Left,
    Right,
    /// Other special keys
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Backspace,
    Tab,
    Enter,
}

impl KeyBinding {
    /// Convert to the actual character code(s) this key would produce
    pub fn to_chars(&self) -> Vec<char> {
        match self {
            KeyBinding::Char(c) => vec![*c],
            KeyBinding::Control(c) => {
                let ctrl = ((*c).to_ascii_uppercase() as u8) & 0x1f;
                vec![ctrl as char]
            }
            KeyBinding::Meta(c) => vec!['\x1b', *c],
            KeyBinding::Escape => vec!['\x1b'],
            KeyBinding::Function(n) => {
                // Terminal-dependent - using common xterm sequences
                vec!['\x1b', 'O', ('P' as u8 + n - 1) as char]
            }
            KeyBinding::Up => vec!['\x1b', '[', 'A'],
            KeyBinding::Down => vec!['\x1b', '[', 'B'],
            KeyBinding::Right => vec!['\x1b', '[', 'C'],
            KeyBinding::Left => vec!['\x1b', '[', 'D'],
            KeyBinding::Home => vec!['\x1b', '[', 'H'],
            KeyBinding::End => vec!['\x1b', '[', 'F'],
            KeyBinding::PageUp => vec!['\x1b', '[', '5', '~'],
            KeyBinding::PageDown => vec!['\x1b', '[', '6', '~'],
            KeyBinding::Insert => vec!['\x1b', '[', '2', '~'],
            KeyBinding::Delete => vec!['\x1b', '[', '3', '~'],
            KeyBinding::Backspace => vec!['\x7f'],
            KeyBinding::Tab => vec!['\t'],
            KeyBinding::Enter => vec!['\r'],
        }
    }

    /// Display string representation
    pub fn to_display_string(&self) -> String {
        match self {
            KeyBinding::Char(c) if c.is_control() => {
                format!("^{}", ((*c as u8) + 64) as char)
            }
            KeyBinding::Char(c) => c.to_string(),
            KeyBinding::Control(c) => format!("^{}", c.to_ascii_uppercase()),
            KeyBinding::Meta(c) => format!("\\e{}", c),
            KeyBinding::Escape => "\\e".to_string(),
            KeyBinding::Function(n) => format!("F{}", n),
            KeyBinding::Up => "\\e[A".to_string(),
            KeyBinding::Down => "\\e[B".to_string(),
            KeyBinding::Right => "\\e[C".to_string(),
            KeyBinding::Left => "\\e[D".to_string(),
            KeyBinding::Home => "\\e[H".to_string(),
            KeyBinding::End => "\\e[F".to_string(),
            KeyBinding::PageUp => "\\e[5~".to_string(),
            KeyBinding::PageDown => "\\e[6~".to_string(),
            KeyBinding::Insert => "\\e[2~".to_string(),
            KeyBinding::Delete => "\\e[3~".to_string(),
            KeyBinding::Backspace => "^?".to_string(),
            KeyBinding::Tab => "^I".to_string(),
            KeyBinding::Enter => "^M".to_string(),
        }
    }

    /// Check if this matches a raw character input
    pub fn matches_char(&self, c: char) -> bool {
        match self {
            KeyBinding::Char(ch) => *ch == c,
            KeyBinding::Control(ch) => {
                let ctrl = ((ch.to_ascii_uppercase() as u8) & 0x1f) as char;
                ctrl == c
            }
            KeyBinding::Backspace => c == '\x7f' || c == '\x08',
            KeyBinding::Tab => c == '\t',
            KeyBinding::Enter => c == '\r' || c == '\n',
            KeyBinding::Escape => c == '\x1b',
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_sequence_parse() {
        let seq = KeySequence::parse("^X^U");
        assert_eq!(seq.0.len(), 2);
        assert_eq!(seq.0[0], KeyBinding::Control('X'));
        assert_eq!(seq.0[1], KeyBinding::Control('U'));
    }

    #[test]
    fn test_key_binding_to_chars() {
        assert_eq!(KeyBinding::Control('A').to_chars(), vec!['\x01']);
        assert_eq!(KeyBinding::Meta('x').to_chars(), vec!['\x1b', 'x']);
    }
}
