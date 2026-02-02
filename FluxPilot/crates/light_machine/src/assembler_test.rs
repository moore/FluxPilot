use crate::assembler::{Assembler, AssemblerError, AssemblerErrorKind};
use crate::builder::ProgramBuilder;

#[test]
fn assembles_basic_program() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 2>::new(&mut buffer, 1, 1, 0).unwrap();
    let mut asm: Assembler<1, 2, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main locals 3 functions 2").unwrap();
    asm.add_line(".func set_rgb index 0").unwrap();
    asm.add_line("LSTORE 0").unwrap();
    asm.add_line("LSTORE 1").unwrap();
    asm.add_line("LSTORE 2").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".func get_rgb index 1").unwrap();
    asm.add_line("LLOAD 0").unwrap();
    asm.add_line("LLOAD 1").unwrap();
    asm.add_line("LLOAD 2").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.instances.len(), 1);
    assert!(descriptor.length > 0);
}

#[test]
fn supports_forward_function_declaration() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 2>::new(&mut buffer, 1, 1, 0).unwrap();
    let mut asm: Assembler<1, 2, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main locals 1 functions 2").unwrap();
    asm.add_line(".func_decl later index 1").unwrap();
    asm.add_line(".func first index 0").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".func later").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.instances.len(), 1);
}

#[test]
fn reports_line_numbers_on_error() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 0).unwrap();
    let mut asm: Assembler<1, 1, 8, 8> = Assembler::new(builder);

    asm.add_line(".machine main locals 1 functions 1").unwrap();
    asm.add_line(".func test index 0").unwrap();
    let err = asm.add_line("BADOP").unwrap_err();
    match err {
        AssemblerError::WithLine { line, kind } => {
            assert_eq!(line, 3);
            assert!(matches!(kind, AssemblerErrorKind::InvalidInstruction));
        }
        _ => panic!("expected line-numbered error"),
    }
}

#[test]
fn supports_named_globals() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 0).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main locals 2 functions 1").unwrap();
    asm.add_line(".local red 0").unwrap();
    asm.add_line(".local green 1").unwrap();
    asm.add_line(".func set index 0").unwrap();
    asm.add_line("LSTORE red").unwrap();
    asm.add_line("LSTORE green").unwrap();
    asm.add_line("LLOAD red").unwrap();
    asm.add_line("LLOAD green").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.instances.len(), 1);
}

#[test]
fn supports_shared_functions_and_data() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 2>::new(&mut buffer, 1, 1, 1).unwrap();
    let mut asm: Assembler<1, 2, 16, 16> = Assembler::new(builder);
    asm.add_line(".shared shared0 0").unwrap();
    asm.add_line(".shared_data shared_data").unwrap();
    asm.add_line("shared_word:").unwrap();
    asm.add_line(".word 7").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".shared_func helper index 0").unwrap();
    asm.add_line("GLOAD shared0").unwrap();
    asm.add_line("LOAD_STATIC shared_word").unwrap();
    asm.add_line("ADD").unwrap();
    asm.add_line("RET 1").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".machine main locals 0 functions 1").unwrap();
    asm.add_line(".func main index 0").unwrap();
    asm.add_line("PUSH 0").unwrap();
    asm.add_line("CALL_SHARED helper").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();
    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.instances.len(), 1);
}

#[test]
fn shared_function_requires_declaration() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 1).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);
    asm.add_line(".shared_func_decl helper index 0").unwrap();
    let err = asm.finish().unwrap_err();
    assert!(matches!(
        err,
        AssemblerError::Kind(AssemblerErrorKind::FunctionNotDeclared)
    ));
}

#[test]
fn shared_function_rejects_out_of_range_shared_global() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 1).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);
    asm.add_line(".shared shared0 0").unwrap();
    asm.add_line(".shared_func helper index 0").unwrap();
    let err = asm.add_line("GLOAD 1").unwrap_err();
    assert!(matches!(
        err,
        AssemblerError::WithLine {
            kind: AssemblerErrorKind::GlobalIndexOutOfRange,
            ..
        }
    ));
}

#[test]
fn shared_data_rejects_duplicate_label() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 1).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);
    asm.add_line(".shared_data shared_data").unwrap();
    asm.add_line("dup:").unwrap();
    let err = asm.add_line("dup:").unwrap_err();
    assert!(matches!(
        err,
        AssemblerError::WithLine {
            kind: AssemblerErrorKind::DuplicateLabel,
            ..
        }
    ));
}

#[test]
fn shared_function_cannot_be_started_inside_machine() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 1).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);
    asm.add_line(".machine main locals 0 functions 1").unwrap();
    let err = asm.add_line(".shared_func helper index 0").unwrap_err();
    assert!(matches!(
        err,
        AssemblerError::WithLine {
            kind: AssemblerErrorKind::UnexpectedDirective,
            ..
        }
    ));
}

#[test]
fn shared_func_decl_duplicate_index_is_error() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 2).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);
    asm.add_line(".shared_func_decl a index 0").unwrap();
    let err = asm.add_line(".shared_func_decl b index 0").unwrap_err();
    assert!(matches!(
        err,
        AssemblerError::WithLine {
            kind: AssemblerErrorKind::FunctionIndexDuplicate,
            ..
        }
    ));
}

#[test]
fn shared_function_label_not_visible_in_machine() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 1).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);
    asm.add_line(".shared_func helper index 0").unwrap();
    asm.add_line("shared_label:").unwrap();
    asm.add_line("RET 0").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".machine main locals 0 functions 1").unwrap();
    asm.add_line(".func main index 0").unwrap();
    asm.add_line("JUMP shared_label").unwrap();
    let err = asm.add_line(".end").unwrap_err();
    assert!(matches!(
        err,
        AssemblerError::WithLine {
            kind: AssemblerErrorKind::UnknownLabel,
            ..
        }
    ));
}

#[test]
fn supports_shared_globals() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 0).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);

    asm.add_line(".shared brightness 0").unwrap();
    asm.add_line(".machine main locals 1 functions 1").unwrap();
    asm.add_line(".func main index 0").unwrap();
    asm.add_line("GLOAD brightness").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.instances.len(), 1);
}

#[test]
fn supports_named_stack_slots() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1, 1, 0).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main locals 0 functions 1").unwrap();
    asm.add_line(".func main index 0").unwrap();
    asm.add_line(".frame first 0").unwrap();
    asm.add_line(".frame second 1").unwrap();
    asm.add_line("SLOAD first").unwrap();
    asm.add_line("SSTORE second").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.instances.len(), 1);
}
