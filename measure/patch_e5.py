def sub(p, a, b, count=1):
    s = open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')
    assert s.count(a) == count, (p, s.count(a), a[:100])
    open(p, 'w', encoding='utf-8', newline='').write(s.replace(a, b))


H = 'crates/unisql-differential/src/lib.rs'
sub(H, "    pub constructs: Vec<(String, Tier)>,\n", "    pub constructs: Vec<(String, Tier)>,\n    /// `(construct, allowance)` for every Conditional entry the report recorded.\n    pub allowances: Vec<(String, Allowance)>,\n")
sub(H, "constructs: vec![], refusal: None, rows: 0 };", "constructs: vec![], allowances: vec![], refusal: None, rows: 0 };")
sub(H, "            out.constructs = t.report.entries.iter().map(|x| (x.construct.clone(), x.tier)).collect();",
    "            out.constructs = t.report.entries.iter().map(|x| (x.construct.clone(), x.tier)).collect();\n            out.allowances = t.report.entries.iter().filter_map(|x| x.allowance.map(|a| (x.construct.clone(), a))).collect();")
s = open(H, encoding='utf-8', newline='').read().replace('\r\n', '\n')
i = s.index('/// Matrix rows in PersistAnt')
new = '''/// Matrix rows in PersistAnt's agreed schema (agreed with the PersistAnt lane 2026-10-12):
/// `subject_kind, subject, backend, version, how, evidence, note, from`.
///
/// - `how`: `native` (Exact), `emulated` (Emulated), `conditional` (Conditional: the construct
///   translates only if the call site names the allowance, listed as `allowance=a,b` at the start of
///   `note`, so a gate can read it), `absent` (refused). Weakest proven form wins per subject.
/// - `evidence`: `proven` only for a construct that translated and whose result matched on the real
///   engines; a refusal is `declared` (the translator refuses; the engine's lack is not itself tested).
/// - A refused *form* of a construct that also has a proven form is its own subject
///   `<construct>.refused`, never a note on the proven cell.
pub fn matrix_tsv(outcomes: &[Outcome], backend: &str, version: &str, from: &str) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let rank = |h: &str| match h {
        "native" => 0,
        "emulated" => 1,
        "conditional" => 2,
        _ => 3,
    };
    let mut proven: BTreeMap<String, (String, BTreeSet<&'static str>)> = BTreeMap::new();
    for o in outcomes.iter().filter(|o| o.passed) {
        for (c, t) in &o.constructs {
            let how = match t {
                Tier::Exact => "native",
                Tier::Emulated => "emulated",
                _ => "conditional",
            };
            let e = proven.entry(c.clone()).or_insert((how.to_string(), BTreeSet::new()));
            if rank(how) > rank(&e.0) {
                e.0 = how.to_string();
            }
        }
        for (c, a) in &o.allowances {
            if let Some(e) = proven.get_mut(c) {
                e.1.insert(a.name());
            }
        }
    }
    let mut refused: BTreeMap<String, String> = BTreeMap::new();
    for o in outcomes.iter().filter(|o| o.passed) {
        if let Some((c, why)) = &o.refusal {
            refused.entry(c.clone()).or_insert_with(|| why.replace(['\\t', '\\n'], " "));
        }
    }
    let mut out = String::from("subject_kind\\tsubject\\tbackend\\tversion\\thow\\tevidence\\tnote\\tfrom\\n");
    let mut row = |subject: &str, how: &str, ev: &str, note: &str| {
        out.push_str(&format!("construct\\t{subject}\\t{backend}\\t{version}\\t{how}\\t{ev}\\t{note}\\t{from}\\n"));
    };
    let mut subjects: BTreeSet<String> = proven.keys().cloned().collect();
    subjects.extend(refused.keys().cloned());
    for s in subjects {
        match (proven.get(&s), refused.get(&s)) {
            (Some((how, al)), r) => {
                let note = if al.is_empty() {
                    String::new()
                } else {
                    format!("allowance={}", al.iter().copied().collect::<Vec<_>>().join(","))
                };
                row(&s, how, "proven", &note);
                if let Some(why) = r {
                    row(&format!("{s}.refused"), "absent", "declared", why);
                }
            }
            (None, Some(why)) => row(&s, "absent", "declared", why),
            (None, None) => {}
        }
    }
    out
}
'''
s = s[:i] + new
open(H, 'w', encoding='utf-8', newline='').write(s)
print('ok')
