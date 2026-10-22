//! A minimal X11 "text field" for the Xvfb integration test.
//!
//! It takes the input focus, turns each key press into a character the way a
//! classic Xlib client does (core keyboard mapping + the XKB group and Shift
//! bits of the event state), handles BackSpace, and prints its whole text
//! as `TEXT:<text>` on stdout after every change. It prints `READY` once it
//! has the focus. The window title shows the text too.
//!
//! It deliberately does not share code with the backend, so the test checks
//! what an ordinary client would see.
#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("lf-x11-testfield runs on Linux X11 only");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(e) = x11::run() {
        eprintln!("lf-x11-testfield: {e}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::error::Error;
    use std::io::Write;

    use x11rb::connection::Connection;
    use x11rb::protocol::xkb::ConnectionExt as _;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, PropMode, WindowClass,
    };
    use x11rb::protocol::Event;
    use x11rb::wrapper::ConnectionExt as _;

    const XK_BACKSPACE: u32 = 0xff08;
    const CLASS: &[u8] = b"lf-x11-testfield\0lf-x11-testfield\0";

    /// The core keyboard mapping.
    struct Mapping {
        min: u8,
        per: usize,
        syms: Vec<u32>,
    }

    impl Mapping {
        fn read(conn: &impl Connection) -> Result<Self, Box<dyn Error>> {
            let setup = conn.setup();
            let (min, max) = (setup.min_keycode, setup.max_keycode);
            let reply = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
            Ok(Mapping {
                min,
                per: usize::from(reply.keysyms_per_keycode),
                syms: reply.keysyms,
            })
        }

        /// Keysym for a key in a group (0-based) with or without Shift.
        /// Columns are G1L1 G1L2 G2L1 G2L2; missing entries fall back to
        /// group 1 and then to the unshifted level.
        fn keysym(&self, keycode: u8, group: usize, shift: bool) -> u32 {
            let Some(row) = keycode
                .checked_sub(self.min)
                .map(|i| usize::from(i) * self.per)
                .and_then(|start| self.syms.get(start..start + self.per))
            else {
                return 0;
            };
            let at = |col: usize| row.get(col).copied().unwrap_or(0);
            let group = if group < 2 && at(group * 2) != 0 {
                group
            } else {
                0
            };
            let sym = at(group * 2 + usize::from(shift));
            if sym == 0 {
                at(group * 2)
            } else {
                sym
            }
        }
    }

    fn print(line: &str) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        // Become XKB-aware so key events carry the layout group.
        conn.xkb_use_extension(1, 0)?.reply()?;
        let root = conn.setup().roots[screen].root;
        let win = conn.generate_id()?;
        conn.create_window(
            0,
            win,
            root,
            10,
            10,
            400,
            80,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(
                EventMask::KEY_PRESS | EventMask::STRUCTURE_NOTIFY | EventMask::EXPOSURE,
            ),
        )?;
        conn.change_property8(
            PropMode::REPLACE,
            win,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            CLASS,
        )?;
        let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME")?.reply()?.atom;
        let utf8 = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
        conn.map_window(win)?;
        conn.flush()?;

        let mut mapping = Mapping::read(&conn)?;
        let mut text = String::new();
        loop {
            match conn.wait_for_event()? {
                Event::MapNotify(e) if e.window == win => {
                    conn.set_input_focus(InputFocus::PARENT, win, x11rb::CURRENT_TIME)?;
                    conn.get_input_focus()?.reply()?;
                    print("READY");
                }
                Event::MappingNotify(_) => mapping = Mapping::read(&conn)?,
                Event::KeyPress(e) => {
                    let state = u16::from(e.state);
                    let shift = state & 1 != 0;
                    let group = usize::from((state >> 13) & 3);
                    let sym = mapping.keysym(e.detail, group, shift);
                    let changed = if sym == XK_BACKSPACE {
                        text.pop().is_some()
                    } else {
                        match char::from_u32(xkbcommon::xkb::keysym_to_utf32(sym.into())) {
                            Some(c) if c != '\0' && !c.is_control() => {
                                text.push(c);
                                true
                            }
                            _ => false,
                        }
                    };
                    if changed {
                        print(&format!("TEXT:{text}"));
                        conn.change_property8(
                            PropMode::REPLACE,
                            win,
                            net_wm_name,
                            utf8,
                            text.as_bytes(),
                        )?;
                        conn.flush()?;
                    }
                }
                Event::DestroyNotify(e) if e.window == win => return Ok(()),
                _ => {}
            }
        }
    }
}
