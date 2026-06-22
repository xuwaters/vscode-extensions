//! Go-to-definition. A task reference (`dependencies`, `run_task`, or an
//! `alias`) jumps to the header of the referenced `[tasks.NAME]` table when
//! that task is defined in the same file.

use crate::ast::File;
use crate::spans::ByteSpan;

pub fn definition(file: &File, offset: u32) -> Option<ByteSpan> {
    for task in &file.tasks {
        for r in task.dependencies.iter().chain(task.references.iter()) {
            if r.span.contains(offset) {
                if let Some(target) = file.task(&r.name) {
                    return Some(target.name_span);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::vfs::FileUri;

    #[test]
    fn jumps_from_dependency_to_task() {
        let src = "[tasks.a]\ncommand = \"x\"\n[tasks.b]\ndependencies = [\"a\"]\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let offset = src.rfind("\"a\"").unwrap() as u32 + 1;
        let target = definition(&pf.ast, offset).unwrap();
        let name = &src[target.start as usize..target.end as usize];
        assert_eq!(name, "a");
    }

    #[test]
    fn unknown_reference_has_no_definition() {
        let src = "[tasks.b]\ndependencies = [\"missing\"]\n";
        let pf = parse(FileUri::new("t"), src.to_string());
        let offset = src.rfind("missing").unwrap() as u32 + 1;
        assert!(definition(&pf.ast, offset).is_none());
    }
}
