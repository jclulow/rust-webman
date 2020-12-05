use std::collections::VecDeque;

#[derive(Debug)]
pub struct Typewriter {
    page: Vec<Vec<Cell>>,
    col: usize,
}

#[derive(Debug, Clone)]
struct Cell {
    c: char,
    b: bool,
    u: bool,
}

impl Typewriter {
    pub fn new() -> Typewriter {
        Typewriter {
            page: vec![Vec::new()],
            col: 0,
        }
    }

    pub fn to_html(&mut self, include_br: bool) -> String {
        let mut html = String::new();

        for l in self.page.iter_mut() {
            let mut stk: VecDeque<char> = VecDeque::new();

            let mut append = |c: &Cell| {
                /*
                 * First, clear any attributes that we are not asserting:
                 */
                if !c.b {
                    while stk.contains(&'b') {
                        html.push_str(&format!("</{}>",
                            stk.pop_back().unwrap()));
                    }
                }
                if !c.u {
                    while stk.contains(&'u') {
                        html.push_str(&format!("</{}>",
                            stk.pop_back().unwrap()));
                    }
                }

                /*
                 * Then, enable any attributes that we are:
                 */
                if c.b && !stk.contains(&'b') {
                    stk.push_back('b');
                    html.push_str("<b>");
                }
                if c.u && !stk.contains(&'u') {
                    stk.push_back('u');
                    html.push_str("<u>");
                }

                /*
                 * Then emit the character for this cell:
                 */
                if c.c == '<' {
                    html.push_str("&lt;");
                } else if c.c == '>' {
                    html.push_str("&gt;");
                } else if c.c == '&' {
                    html.push_str("&amp;");
                } else {
                    html.push(c.c);
                }
            };

            /*
             * Use a sliding window of three adjacent characters so that we can
             * detect runs of bold or underline formatting that span an
             * otherwise unformatted space.  Because the window iterator will
             * only give us 3-wide slices, we must manually handle the first
             * character in the line -- and, if there are at least three
             * characters, the last.
             */
            if let Some(f) = l.first() {
                append(f);
            }
            for cw in l.windows(3) {
                let uu = cw[0].u || cw[1].u || cw[2].u;
                let bb = cw[0].b || cw[1].b || cw[2].b;

                /*
                 * If there is no underlining, and there is an unbolded space
                 * between two bold characters, make the space bold as well:
                 */
                if !uu && cw[0].b && cw[1].c == ' ' && !cw[1].b && cw[2].b {
                    let mut cc = cw[1].clone();
                    cc.b = true;
                    append(&cc);
                    continue;
                }

                /*
                 * If there is no bold, and there is a space between two
                 * underlined characters, make the space underlined as well:
                 */
                if !bb && cw[0].u && cw[1].c == ' ' && !cw[1].u && cw[2].u {
                    let mut cc = cw[1].clone();
                    cc.u = true;
                    append(&cc);
                    continue;
                }

                /*
                 * Otherwise, append the character as-is:
                 */
                append(&cw[1]);
            }
            if l.len() > 1 {
                if let Some(f) = l.last() {
                    append(f);
                }
            }

            while let Some(attr) = stk.pop_back() {
                html.push_str(&format!("</{}>", attr));
            }

            if include_br {
                html.push_str("<br>\n");
            } else {
                html.push('\n');
            }
        }

        html
    }

    pub fn append(&mut self, c: char) {
        if c == '\x08' /* BS */ {
            if self.col > 0 {
                self.col -= 1;
            }
            return;
        }

        if c == '\r' {
            self.col = 0;
            return;
        }

        if c == '\n' {
            self.col = 0;
            self.page.push(Vec::new());
            return;
        }

        if c.is_control() {
            /*
             * Ignore other control characters.
             */
            return;
        }

        let line = self.page.last_mut().unwrap();

        if let Some(cell) = line.get_mut(self.col) {
            if c == '_' && cell.c != '_' {
                cell.u = true;
            } else if c != '_' && cell.c == '_' {
                /*
                 * The underscore was typed first, but we now adopt it as an
                 * underline.
                 */
                cell.c = c;
                cell.u = true;
            } else if c == cell.c {
                cell.b = true;
            } else {
                /*
                 * Would that we were able to represent an overtyped mess.
                 */
                cell.c = c;
            }
        } else {
            line.push(Cell { c, b: false, u: false });
        }
        self.col += 1;
    }
}

#[cfg(test)]
mod test {
    use super::Typewriter;

    #[test]
    fn optarg() {
        let mut t = Typewriter::new();
        let s = "user";
        t.append('[');
        for c in s.chars() {
            t.append('_');
            t.append('\x08');
            t.append(c);
        }
        t.append(']');
        println!("{:#?}", t);
        assert_eq!(t.to_html(false), "[<u>user</u>]\n");
    }

    #[test]
    fn lc_messages() {
        let mut t = Typewriter::new();
        let s = "LC_MESSAGES";
        for c in s.chars() {
            t.append(c);
            t.append('\x08');
            t.append(c);
        }
        println!("{:#?}", t);
        assert_eq!(t.to_html(false), "<b>LC_MESSAGES</b>\n");
    }

    #[test]
    fn bold_with_spaces() {
        let mut t = Typewriter::new();
        let s1 = "ENVIRONMENT";
        let s2 = "VARIABLES";
        for c in s1.chars() {
            t.append(c);
            t.append('\x08');
            t.append(c);
        }
        t.append(' ');
        for c in s2.chars() {
            t.append(c);
            t.append('\x08');
            t.append(c);
        }
        println!("{:#?}", t);
        assert_eq!(t.to_html(false), "<b>ENVIRONMENT VARIABLES</b>\n");
    }

    #[test]
    fn basic() {
        let mut t = Typewriter::new();
        t.append('h');
        t.append('e');
        t.append('l');
        t.append('l');
        t.append('o');
        t.append('\n');

        t.append('w');
        t.append('\x08');
        t.append('_');
        t.append('o');
        t.append('\x08');
        t.append('_');
        t.append('r');
        t.append('\x08');
        t.append('_');
        t.append('l');
        t.append('\x08');
        t.append('_');
        t.append('d');
        t.append('\x08');
        t.append('_');
        t.append('\n');

        t.append('i');
        t.append('n');
        t.append(' ');
        t.append('b');
        t.append('\x08');
        t.append('b');
        t.append('o');
        t.append('\x08');
        t.append('o');
        t.append('l');
        t.append('\x08');
        t.append('l');
        t.append('d');
        t.append('\x08');
        t.append('d');
        t.append('\n');

        t.append('\n');

        t.append('f');
        t.append('\x08');
        t.append('f');
        t.append('\x08');
        t.append('_');
        t.append('i');
        t.append('\x08');
        t.append('i');
        t.append('\x08');
        t.append('_');
        t.append('n');
        t.append('\x08');
        t.append('_');
        t.append('\x08');
        t.append('n');

        println!("{:#?}", t);
        assert_eq!(t.to_html(true),
            "hello<br>\n\
            <u>world</u><br>\n\
            in <b>bold</b><br>\n\
            <br>\n\
            <b><u>fin</u></b><br>\n");
    }
}
