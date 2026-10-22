//! The active XKB keyboard mapping, turned into engine layouts.
//!
//! The X server publishes the names it compiled its keymap from (rules,
//! model, layouts, variants, options) in the `_XKB_RULES_NAMES` root window
//! property; `setxkbmap`, GNOME and KDE all keep it current. We compile the
//! same names with `xkbcommon` and read every key of every group into a plain
//! [`Keymap`] snapshot (xkbcommon objects are not `Send`, the snapshot is).

use std::collections::HashMap;

use lf_core::{KeyCode, Layout, LayoutId, Modifiers};
use xkbcommon::xkb;

use super::keycodes;
use crate::{BackendError, LayoutInfo};

/// `BackSpace` keysym.
const XK_BACKSPACE: u32 = 0xff08;
/// `Shift_L` keysym.
const XK_SHIFT_L: u32 = 0xffe1;
/// `Shift_R` keysym.
const XK_SHIFT_R: u32 = 0xffe2;
/// `ISO_Level3_Shift` keysym (AltGr).
const XK_ISO_LEVEL3_SHIFT: u32 = 0xfe03;

/// The names the X server built its keymap from (`_XKB_RULES_NAMES`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RulesNames {
    pub rules: String,
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: String,
}

impl RulesNames {
    /// Parse the property value: five NUL-terminated strings. None if no
    /// layout is named.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut parts = bytes
            .split(|&b| b == 0)
            .map(|p| String::from_utf8_lossy(p).into_owned());
        let mut next = || parts.next().unwrap_or_default();
        let names = RulesNames {
            rules: next(),
            model: next(),
            layout: next(),
            variant: next(),
            options: next(),
        };
        (!names.layout.trim().is_empty()).then_some(names)
    }

    /// `(layout, variant)` for each group, in group order.
    pub fn groups(&self) -> Vec<(String, String)> {
        let mut variants = self.variant.split(',').map(str::trim);
        self.layout
            .split(',')
            .map(|l| {
                (
                    l.trim().to_owned(),
                    variants.next().unwrap_or("").to_owned(),
                )
            })
            .collect()
    }
}

/// Two-letter language of an XKB layout, if we know it.
pub(crate) fn language_for(layout: &str, variant: &str) -> Option<&'static str> {
    match layout {
        "us" | "gb" | "au" | "ie" | "nz" | "za" => Some("en"),
        "ir" => matches!(variant, "" | "pes" | "pes_keypad" | "winkeys").then_some("fa"),
        "af" => matches!(variant, "" | "fa-olpc").then_some("fa"),
        "ara" => Some("ar"),
        "de" | "at" => Some("de"),
        "fr" => Some("fr"),
        "es" | "latam" => Some("es"),
        "it" => Some("it"),
        "ru" => Some("ru"),
        "tr" => Some("tr"),
        _ => None,
    }
}

/// Engine layout ids for the groups: the XKB layout name ("us", "ir"), with
/// the variant added only where two groups share a layout ("us(intl)").
pub(crate) fn layout_ids(groups: &[(String, String)]) -> Vec<String> {
    let mut ids: Vec<String> = groups
        .iter()
        .map(|(layout, variant)| {
            let shared = groups.iter().filter(|(l, _)| l == layout).count() > 1;
            if shared && !variant.is_empty() {
                format!("{layout}({variant})")
            } else {
                layout.clone()
            }
        })
        .collect();
    // Two identical groups: number the later ones.
    for i in 1..ids.len() {
        if ids[..i].contains(&ids[i]) {
            ids[i] = format!("{}#{}", ids[i], i + 1);
        }
    }
    ids
}

/// Which modifiers a key is pressed with, as far as the character goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Level {
    /// Shift held (or latched).
    pub shift: bool,
    /// AltGr / ISO_Level3_Shift held.
    pub lvl3: bool,
    /// Caps Lock on.
    pub lock: bool,
}

impl Level {
    fn index(self) -> usize {
        usize::from(self.shift) | usize::from(self.lvl3) << 1 | usize::from(self.lock) << 2
    }

    fn from_index(i: usize) -> Self {
        Level {
            shift: i & 1 != 0,
            lvl3: i & 2 != 0,
            lock: i & 4 != 0,
        }
    }
}

/// How to type one character: press `keycode` with these modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stroke {
    /// X keycode.
    pub keycode: u8,
    /// Hold Shift.
    pub shift: bool,
    /// Hold AltGr.
    pub lvl3: bool,
}

#[derive(Debug, Clone)]
struct Group {
    info: LayoutInfo,
    shift_key: Option<u8>,
    lvl3_key: Option<u8>,
}

/// A plain-data snapshot of the X server's keymap.
#[derive(Debug, Clone)]
pub(crate) struct Keymap {
    names: RulesNames,
    groups: Vec<Group>,
    /// Char per (keycode, group), indexed by [`Level::index`].
    chars: HashMap<(u8, u8), [Option<char>; 8]>,
    backspace: u8,
}

impl Keymap {
    /// Compile the keymap these names describe.
    pub fn compile(names: &RulesNames) -> Result<Self, BackendError> {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_ENVIRONMENT_NAMES);
        let km = xkb::Keymap::new_from_names(
            &ctx,
            &names.rules,
            &names.model,
            &names.layout,
            &names.variant,
            (!names.options.is_empty()).then(|| names.options.clone()),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| {
            BackendError::Connection(format!(
                "cannot compile the XKB keymap for layouts \"{}\"",
                names.layout
            ))
        })?;

        let mask = |name: &str| match km.mod_get_index(name) {
            xkb::MOD_INVALID => 0,
            i => 1u32 << i,
        };
        let shift = mask(xkb::MOD_NAME_SHIFT);
        let lvl3 = mask(xkb::MOD_NAME_ISO_LEVEL3_SHIFT);
        let lock = mask(xkb::MOD_NAME_CAPS);

        let n_groups = km.num_layouts().min(4) as u8;
        let mut named = names.groups();
        named.resize(usize::from(n_groups), (String::new(), String::new()));
        named.truncate(usize::from(n_groups));
        let ids = layout_ids(&named);

        let min = km.min_keycode().raw().max(8);
        let max = km.max_keycode().raw().min(255);
        let mut chars = HashMap::new();
        let mut state = xkb::State::new(&km);
        for g in 0..n_groups {
            for level in 0..8 {
                let l = Level::from_index(level);
                let depressed = if l.shift { shift } else { 0 } | if l.lvl3 { lvl3 } else { 0 };
                let locked = if l.lock { lock } else { 0 };
                state.update_mask(depressed, 0, locked, 0, 0, u32::from(g));
                for kc in min..=max {
                    let ch = char::from_u32(state.key_get_utf32(xkb::Keycode::new(kc)))
                        .filter(|c| *c != '\0' && !c.is_control());
                    let entry = chars.entry((kc as u8, g)).or_insert([None; 8]);
                    entry[level] = ch;
                }
            }
        }
        chars.retain(|_, v| v.iter().any(Option::is_some));

        let key_with = |g: u8, sym: u32| {
            (min..=max).find_map(|kc| {
                km.key_get_syms_by_level(xkb::Keycode::new(kc), u32::from(g), 0)
                    .iter()
                    .any(|s| s.raw() == sym)
                    .then_some(kc as u8)
            })
        };
        let backspace =
            key_with(0, XK_BACKSPACE).unwrap_or(keycodes::BACKSPACE as u8 + keycodes::EVDEV_OFFSET);

        let mut groups = Vec::new();
        for g in 0..n_groups {
            let (layout_name, variant) = &named[usize::from(g)];
            let lvl3_key = key_with(g, XK_ISO_LEVEL3_SHIFT);
            let shift_key = key_with(g, XK_SHIFT_L).or_else(|| key_with(g, XK_SHIFT_R));
            let at = |code: KeyCode, l: Level| {
                chars
                    .get(&(keycodes::x_keycode(code), g))
                    .and_then(|c| c[l.index()])
            };
            let mut entries = Vec::new();
            for &code in KeyCode::ALL {
                for shifted in [false, true] {
                    let base = at(
                        code,
                        Level {
                            shift: shifted,
                            ..Level::default()
                        },
                    );
                    let mods = Modifiers {
                        shift: shifted,
                        altgr: false,
                    };
                    if let Some(ch) = base {
                        entries.push((code, mods, ch));
                    }
                    if lvl3_key.is_some() {
                        let third = at(
                            code,
                            Level {
                                shift: shifted,
                                lvl3: true,
                                lock: false,
                            },
                        );
                        if let Some(ch) = third.filter(|&c| Some(c) != base) {
                            entries.push((
                                code,
                                Modifiers {
                                    shift: shifted,
                                    altgr: true,
                                },
                                ch,
                            ));
                        }
                    }
                }
            }
            let id = &ids[usize::from(g)];
            let name = km.layout_get_name(u32::from(g));
            let name = if name.is_empty() { id.as_str() } else { name };
            groups.push(Group {
                info: LayoutInfo {
                    layout: Layout::new(id, name, &entries),
                    language: language_for(layout_name, variant).map(str::to_owned),
                },
                shift_key,
                lvl3_key,
            });
        }

        Ok(Keymap {
            names: names.clone(),
            groups,
            chars,
            backspace,
        })
    }

    /// The names this keymap was compiled from.
    pub fn names(&self) -> &RulesNames {
        &self.names
    }

    /// Number of groups (installed layouts).
    pub fn group_count(&self) -> u8 {
        self.groups.len() as u8
    }

    /// Every group as an engine layout, in group order.
    pub fn layouts(&self) -> Vec<LayoutInfo> {
        self.groups.iter().map(|g| g.info.clone()).collect()
    }

    /// The layout id of a group.
    pub fn id(&self, group: u8) -> Option<&LayoutId> {
        self.groups
            .get(usize::from(group))
            .map(|g| &g.info.layout.id)
    }

    /// The group of a layout id.
    pub fn group_of(&self, id: &LayoutId) -> Option<u8> {
        self.groups
            .iter()
            .position(|g| &g.info.layout.id == id)
            .map(|i| i as u8)
    }

    /// True if both keymaps have the same layouts (ids, names, languages).
    pub fn same_layouts(&self, other: &Keymap) -> bool {
        let key = |k: &Keymap| -> Vec<(LayoutId, String, Option<String>)> {
            k.groups
                .iter()
                .map(|g| {
                    (
                        g.info.layout.id.clone(),
                        g.info.layout.name.clone(),
                        g.info.language.clone(),
                    )
                })
                .collect()
        };
        key(self) == key(other)
    }

    /// The char `keycode` types in `group` at `level`, if any.
    pub fn char_for(&self, keycode: u8, group: u8, level: Level) -> Option<char> {
        self.chars.get(&(keycode, group))?[level.index()]
    }

    /// The X keycode of BackSpace.
    pub fn backspace(&self) -> u8 {
        self.backspace
    }

    /// Keycode of a Shift key in `group`.
    pub fn shift_key(&self, group: u8) -> Option<u8> {
        self.groups.get(usize::from(group))?.shift_key
    }

    /// Keycode of the AltGr (ISO_Level3_Shift) key in `group`.
    pub fn lvl3_key(&self, group: u8) -> Option<u8> {
        self.groups.get(usize::from(group))?.lvl3_key
    }

    /// How to type `ch` in `group` with Caps Lock `lock`: the lowest keycode,
    /// with the fewest modifiers, that the group can actually reach.
    pub fn find(&self, ch: char, group: u8, lock: bool) -> Option<Stroke> {
        let has_shift = self.shift_key(group).is_some();
        let has_lvl3 = self.lvl3_key(group).is_some();
        for (shift, lvl3) in [(false, false), (true, false), (false, true), (true, true)] {
            if (shift && !has_shift) || (lvl3 && !has_lvl3) {
                continue;
            }
            let level = Level { shift, lvl3, lock };
            let hit = (8..=255u8).find(|&kc| self.char_for(kc, group, level) == Some(ch));
            if let Some(keycode) = hit {
                return Some(Stroke {
                    keycode,
                    shift,
                    lvl3,
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(layout: &str, variant: &str) -> RulesNames {
        RulesNames {
            rules: "evdev".into(),
            model: "pc105".into(),
            layout: layout.into(),
            variant: variant.into(),
            options: String::new(),
        }
    }

    fn us_ir() -> Keymap {
        Keymap::compile(&names("us,ir", ",")).expect("xkeyboard-config data is installed")
    }

    #[test]
    fn parses_rules_names_property() {
        let n = RulesNames::parse(b"evdev\0pc105\0us,ir\0,pes\0grp:alt_shift_toggle\0").unwrap();
        assert_eq!(n.rules, "evdev");
        assert_eq!(n.layout, "us,ir");
        assert_eq!(n.options, "grp:alt_shift_toggle");
        assert_eq!(
            n.groups(),
            vec![("us".into(), String::new()), ("ir".into(), "pes".into())]
        );
        let short = RulesNames::parse(b"evdev\0\0us\0").unwrap();
        assert_eq!(short.variant, "");
        assert_eq!(short.groups(), vec![("us".into(), String::new())]);
        assert_eq!(RulesNames::parse(b"evdev\0pc105\0\0\0\0"), None);
        assert_eq!(RulesNames::parse(b""), None);
    }

    #[test]
    fn ids_add_variants_only_when_needed() {
        let g = |l: &str, v: &str| (l.to_owned(), v.to_owned());
        assert_eq!(layout_ids(&[g("us", ""), g("ir", "pes")]), ["us", "ir"]);
        assert_eq!(
            layout_ids(&[g("us", ""), g("us", "intl"), g("ir", "")]),
            ["us", "us(intl)", "ir"]
        );
        assert_eq!(layout_ids(&[g("us", ""), g("us", "")]), ["us", "us#2"]);
    }

    #[test]
    fn languages() {
        assert_eq!(language_for("us", ""), Some("en"));
        assert_eq!(language_for("us", "dvorak"), Some("en"));
        assert_eq!(language_for("ir", ""), Some("fa"));
        assert_eq!(language_for("ir", "pes"), Some("fa"));
        assert_eq!(language_for("ir", "ku"), None);
        assert_eq!(language_for("xx", ""), None);
    }

    #[test]
    fn us_ir_keymap_matches_the_builtin_tables() {
        let km = us_ir();
        assert_eq!(km.group_count(), 2);
        let infos = km.layouts();
        assert_eq!(infos[0].layout.id, LayoutId::new("us"));
        assert_eq!(infos[0].language.as_deref(), Some("en"));
        assert_eq!(infos[1].layout.id, LayoutId::new("ir"));
        assert_eq!(infos[1].language.as_deref(), Some("fa"));
        assert_eq!(km.group_of(&LayoutId::new("ir")), Some(1));
        assert_eq!(km.id(1), Some(&LayoutId::new("ir")));

        // Every unshifted/shifted key agrees with the hand-written tables,
        // so render() gives the same text as with the built-ins.
        for (live, builtin) in [
            (&infos[0].layout, lf_layouts::us()),
            (&infos[1].layout, lf_layouts::ir_pes()),
        ] {
            for &code in KeyCode::ALL {
                for mods in [Modifiers::NONE, Modifiers::SHIFT] {
                    assert_eq!(
                        live.char_for(code, mods),
                        builtin.char_for(code, mods),
                        "{} {code:?} {mods:?}",
                        live.id
                    );
                }
            }
        }
    }

    #[test]
    fn char_for_follows_group_shift_and_caps_lock() {
        let km = us_ir();
        let s = keycodes::x_keycode(KeyCode::KeyS);
        let plain = Level::default();
        let shift = Level {
            shift: true,
            ..plain
        };
        let lock = Level {
            lock: true,
            ..plain
        };
        assert_eq!(km.char_for(s, 0, plain), Some('s'));
        assert_eq!(km.char_for(s, 0, shift), Some('S'));
        assert_eq!(km.char_for(s, 0, lock), Some('S'));
        assert_eq!(km.char_for(s, 1, plain), Some('س'));
        assert_eq!(km.char_for(km.backspace(), 0, plain), None);
        assert_eq!(km.backspace(), 22);
    }

    #[test]
    fn find_picks_the_simplest_stroke() {
        let km = us_ir();
        let key = |c: KeyCode| keycodes::x_keycode(c);
        let stroke = |kc, shift| Stroke {
            keycode: kc,
            shift,
            lvl3: false,
        };
        assert_eq!(
            km.find('s', 0, false),
            Some(stroke(key(KeyCode::KeyS), false))
        );
        assert_eq!(
            km.find('S', 0, false),
            Some(stroke(key(KeyCode::KeyS), true))
        );
        // With Caps Lock on, lowercase needs Shift.
        assert_eq!(
            km.find('s', 0, true),
            Some(stroke(key(KeyCode::KeyS), true))
        );
        assert_eq!(
            km.find('ل', 1, false),
            Some(stroke(key(KeyCode::KeyG), false))
        );
        assert_eq!(
            km.find('آ', 1, false),
            Some(stroke(key(KeyCode::KeyH), true))
        );
        // ZWNJ: Shift+Space comes before Shift+B (lower keycode wins among equals
        // only within one level, so check it's one of them).
        let zwnj = km.find('\u{200C}', 1, false).unwrap();
        assert!(zwnj.shift);
        assert_eq!(km.find('س', 0, false), None);
        assert_eq!(km.find('€', 0, false), None);
        assert!(km.shift_key(0).is_some());
    }

    #[test]
    fn altgr_levels_are_read_when_the_group_has_an_altgr_key() {
        let km = Keymap::compile(&names("de", "")).unwrap();
        let q = keycodes::x_keycode(KeyCode::KeyQ);
        assert!(km.lvl3_key(0).is_some());
        assert_eq!(
            km.find('@', 0, false),
            Some(Stroke {
                keycode: q,
                shift: false,
                lvl3: true
            })
        );
        let de = &km.layouts()[0].layout;
        assert_eq!(
            de.char_for(
                KeyCode::KeyQ,
                Modifiers {
                    shift: false,
                    altgr: true
                }
            ),
            Some('@')
        );
        assert_eq!(de.char_for(KeyCode::KeyY, Modifiers::NONE), Some('z'));
    }

    #[test]
    fn same_layouts_compares_ids_and_names() {
        assert!(us_ir().same_layouts(&us_ir()));
        let us = Keymap::compile(&names("us", "")).unwrap();
        assert!(!us.same_layouts(&us_ir()));
    }
}
