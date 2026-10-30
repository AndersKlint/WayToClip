use xkbcommon::xkb;

pub fn chord(keymap: &xkb::Keymap, is_terminal: bool) -> Result<Vec<u32>, String> {
    if keymap.num_layouts() == 0 {
        return Err("Paste unsupported: keyboard keymap has no layouts".into());
    }
    let resolve = |symbol: u32| {
        let mut found = None;
        keymap.key_for_each(|map, key| {
            if found.is_none()
                && key.raw() >= 8
                && (0..map.num_layouts()).all(|layout| {
                    let symbols = map.key_get_syms_by_level(key, layout, 0);
                    symbols.len() == 1 && symbols[0].raw() == symbol
                })
            {
                found = Some(key);
            }
        });
        found.ok_or_else(|| {
            "Paste unsupported: v, Control and Shift must have consistent keycodes across every keyboard layout".to_owned()
        })
    };
    let control = resolve(xkb::keysyms::KEY_Control_L)?;
    let shift = resolve(xkb::keysyms::KEY_Shift_L)?;
    let v = resolve(xkb::keysyms::KEY_v)?;
    for layout in 0..keymap.num_layouts() {
        let mut state = xkb::State::new(keymap);
        state.update_mask(0, 0, 0, 0, 0, layout);
        state.update_key(control, xkb::KeyDirection::Down);
        if !state.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE) {
            return Err("Paste unsupported: Control key does not activate Control".into());
        }
        if is_terminal {
            state.update_key(shift, xkb::KeyDirection::Down);
            if !state.mod_name_is_active(xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE) {
                return Err("Paste unsupported: Shift key does not activate Shift".into());
            }
        }
        let expected = if is_terminal {
            xkb::keysyms::KEY_V
        } else {
            xkb::keysyms::KEY_v
        };
        if state.key_get_one_sym(v).raw() != expected {
            return Err("Paste unsupported: modified v differs between keyboard layouts".into());
        }
    }
    let mut keys = vec![control.raw() - 8];
    if is_terminal {
        keys.push(shift.raw() - 8);
    }
    keys.push(v.raw() - 8);
    Ok(keys)
}
