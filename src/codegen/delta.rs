// delta compilation: track which functions changed between builds
// only recompile changed functions, flash only the diff

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug)]
pub struct DeltaResult {
    pub changed: Vec<String>,
    pub unchanged: Vec<String>,
    pub new: Vec<String>,
    pub removed: Vec<String>,
}

// compare two compiled outputs and return which functions changed
pub fn compute_delta(
    old_labels: &HashMap<String, usize>,
    old_code: &[u8],
    new_labels: &HashMap<String, usize>,
    new_code: &[u8],
) -> DeltaResult {
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    let mut new_fns = Vec::new();
    let mut removed = Vec::new();

    // get function names (labels that don't contain '.')
    let old_fns: Vec<&String> = old_labels.keys().filter(|k| !k.contains('.')).collect();
    let new_fns_set: Vec<&String> = new_labels.keys().filter(|k| !k.contains('.')).collect();

    for name in &new_fns_set {
        if let (Some(&new_start), Some(&old_start)) = (new_labels.get(*name), old_labels.get(*name))
        {
            // function exists in both — compare code
            let new_end = find_next_label(new_labels, new_start, new_code.len());
            let old_end = find_next_label(old_labels, old_start, old_code.len());

            let new_bytes = &new_code[new_start..new_end.min(new_code.len())];
            let old_bytes = &old_code[old_start..old_end.min(old_code.len())];

            if new_bytes == old_bytes {
                unchanged.push(name.to_string());
            } else {
                changed.push(name.to_string());
            }
        } else {
            new_fns.push(name.to_string());
        }
    }

    for name in &old_fns {
        if !new_labels.contains_key(*name) {
            removed.push(name.to_string());
        }
    }

    DeltaResult {
        changed,
        unchanged,
        new: new_fns,
        removed,
    }
}

fn find_next_label(labels: &HashMap<String, usize>, after: usize, max: usize) -> usize {
    labels
        .values()
        .filter(|&&v| v > after)
        .min()
        .copied()
        .unwrap_or(max)
}

// save compilation state for next delta comparison
pub fn save_state(path: &Path, labels: &HashMap<String, usize>, code: &[u8]) {
    let mut data = Vec::new();
    // header: label count
    data.extend_from_slice(&(labels.len() as u32).to_le_bytes());
    for (name, &offset) in labels {
        let name_bytes = name.as_bytes();
        data.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&(offset as u32).to_le_bytes());
    }
    // code
    data.extend_from_slice(&(code.len() as u32).to_le_bytes());
    data.extend_from_slice(code);
    let _ = std::fs::write(path, data);
}

pub fn load_state(path: &Path) -> Option<(HashMap<String, usize>, Vec<u8>)> {
    let data = std::fs::read(path).ok()?;
    let mut pos;

    if data.len() < 4 {
        return None;
    }
    let label_count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    pos = 4;

    let mut labels = HashMap::new();
    for _ in 0..label_count {
        if pos + 4 > data.len() {
            return None;
        }
        let name_len =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if pos + name_len > data.len() {
            return None;
        }
        let name = String::from_utf8_lossy(&data[pos..pos + name_len]).to_string();
        pos += name_len;
        if pos + 4 > data.len() {
            return None;
        }
        let offset =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        labels.insert(name, offset);
    }

    if pos + 4 > data.len() {
        return None;
    }
    let code_len =
        u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;
    if pos + code_len > data.len() {
        return None;
    }
    let code = data[pos..pos + code_len].to_vec();

    Some((labels, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_changed_functions() {
        let mut old_labels = HashMap::new();
        old_labels.insert("main".into(), 0);
        old_labels.insert("helper".into(), 20);
        let old_code = vec![0u8; 40];

        let mut new_labels = HashMap::new();
        new_labels.insert("main".into(), 0);
        new_labels.insert("helper".into(), 20);
        let mut new_code = vec![0u8; 40];
        new_code[25] = 0xFF; // change helper

        let delta = compute_delta(&old_labels, &old_code, &new_labels, &new_code);
        assert!(delta.changed.contains(&"helper".to_string()));
        assert!(delta.unchanged.contains(&"main".to_string()));
    }

    #[test]
    fn detect_new_function() {
        let old_labels: HashMap<String, usize> = [("main".into(), 0)].into();
        let new_labels: HashMap<String, usize> = [("main".into(), 0), ("new_fn".into(), 20)].into();

        let delta = compute_delta(&old_labels, &[0; 20], &new_labels, &[0; 40]);
        assert!(delta.new.contains(&"new_fn".to_string()));
    }
}
