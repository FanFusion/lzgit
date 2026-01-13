use std::{fs, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    Ours,
    Theirs,
    Both,
}

#[derive(Clone, Debug)]
pub struct ConflictBlock {
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ConflictFile {
    pub blocks: Vec<ConflictBlock>,
}

struct ParsedBlock {
    start_line: usize,
    end_line: usize,
    ours: Vec<String>,
    theirs: Vec<String>,
}

pub fn load_conflicts(path: &Path) -> Result<ConflictFile, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let parsed = parse_blocks(&lines);

    Ok(ConflictFile {
        blocks: parsed
            .into_iter()
            .map(|b| ConflictBlock {
                ours: b.ours,
                theirs: b.theirs,
            })
            .collect(),
    })
}

pub fn apply_conflict_resolution(
    path: &Path,
    block_index: usize,
    resolution: ConflictResolution,
) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let blocks = parse_blocks(&lines);

    let Some(target) = blocks.get(block_index) else {
        return Err("Conflict block not found".to_string());
    };

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0usize;
    while i < lines.len() {
        if i == target.start_line {
            match resolution {
                ConflictResolution::Ours => out.extend(target.ours.iter().cloned()),
                ConflictResolution::Theirs => out.extend(target.theirs.iter().cloned()),
                ConflictResolution::Both => {
                    out.extend(target.ours.iter().cloned());
                    if !target.ours.is_empty() && !target.theirs.is_empty() {
                        if out.last().is_some_and(|l| !l.is_empty()) {
                            out.push(String::new());
                        }
                    }
                    out.extend(target.theirs.iter().cloned());
                }
            }
            i = target.end_line;
            continue;
        }
        out.push(lines[i].clone());
        i += 1;
    }

    let new_text = out.join("\n");
    fs::write(path, new_text).map_err(|e| e.to_string())?;
    Ok(())
}

fn parse_blocks(lines: &[String]) -> Vec<ParsedBlock> {
    let mut blocks = Vec::new();
    let mut i = 0usize;

    while i < lines.len() {
        if !lines[i].starts_with("<<<<<<<") {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;

        let mut ours = Vec::new();
        while i < lines.len() && !lines[i].starts_with("=======") {
            ours.push(lines[i].clone());
            i += 1;
        }

        if i >= lines.len() {
            break;
        }
        i += 1;

        let mut theirs = Vec::new();
        while i < lines.len() && !lines[i].starts_with(">>>>>>>") {
            theirs.push(lines[i].clone());
            i += 1;
        }

        if i >= lines.len() {
            break;
        }
        i += 1;

        let end = i;

        blocks.push(ParsedBlock {
            start_line: start,
            end_line: end,
            ours,
            theirs,
        });
    }

    blocks
}
