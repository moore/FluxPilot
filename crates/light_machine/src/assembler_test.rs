use crate::assembler::{Assembler, AssemblerError, AssemblerErrorKind};
use crate::builder::ProgramBuilder;

#[test]
fn assembles_basic_program() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 2>::new(&mut buffer, 1).unwrap();
    let mut asm: Assembler<1, 2, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main globals 3 functions 2").unwrap();
    asm.add_line(".func set_rgb index 0").unwrap();
    asm.add_line("STORE 0").unwrap();
    asm.add_line("STORE 1").unwrap();
    asm.add_line("STORE 2").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".func get_rgb index 1").unwrap();
    asm.add_line("LOAD 0").unwrap();
    asm.add_line("LOAD 1").unwrap();
    asm.add_line("LOAD 2").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.machines.len(), 1);
    assert!(descriptor.length > 0);
}

#[test]
fn supports_forward_function_declaration() {
    let mut buffer = [0u16; 128];
    let builder = ProgramBuilder::<1, 2>::new(&mut buffer, 1).unwrap();
    let mut asm: Assembler<1, 2, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main globals 1 functions 2").unwrap();
    asm.add_line(".func_decl later index 1").unwrap();
    asm.add_line(".func first index 0").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".func later").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.machines.len(), 1);
}

#[test]
fn reports_line_numbers_on_error() {
    let mut buffer = [0u16; 64];
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1).unwrap();
    let mut asm: Assembler<1, 1, 8, 8> = Assembler::new(builder);

    asm.add_line(".machine main globals 1 functions 1").unwrap();
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
    let builder = ProgramBuilder::<1, 1>::new(&mut buffer, 1).unwrap();
    let mut asm: Assembler<1, 1, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine main globals 2 functions 1").unwrap();
    asm.add_line(".global red 0").unwrap();
    asm.add_line(".global green 1").unwrap();
    asm.add_line(".func set index 0").unwrap();
    asm.add_line("STORE red").unwrap();
    asm.add_line("STORE green").unwrap();
    asm.add_line("LOAD red").unwrap();
    asm.add_line("LOAD green").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    assert_eq!(descriptor.machines.len(), 1);
}
