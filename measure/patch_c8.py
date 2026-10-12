p = 'crates/unisql-differential/src/lib.rs'
s = open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')
a = '''        if let Some((c, why)) = &o.refusal {
            worse(c.clone(), "absent", "declared", why.replace('\t', " ").replace('\n', " "));
        }
    }'''
assert s.count(a) == 1, s.count(a)
b = '''    }
    // Refusals last: a construct with a proven translation keeps it and records the refused form.
    for o in outcomes.iter().filter(|o| o.passed) {
        if let Some((c, why)) = &o.refusal {
            let why = why.replace('\t', " ").replace('\n', " ");
            match rows.get_mut(c) {
                Some((_, _, note)) => {
                    if !note.is_empty() {
                        note.push_str("; ");
                    }
                    note.push_str(&format!("refused in one form: {why}"));
                }
                None => {
                    rows.insert(c.clone(), ("absent".into(), "declared".into(), why));
                }
            }
        }
    }'''
s = s.replace(a, b)
open(p, 'w', encoding='utf-8', newline='').write(s)
