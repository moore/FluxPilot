use super::*;
use crate::assembler::Assembler;
use crate::builder::ProgramBuilder;

extern crate std;
use std::println;
use std::vec::Vec as StdVec;

const STACK_CAP: usize = 32;
const ASM_MACHINE_MAX: usize = 1;
const ASM_FUNCTION_MAX: usize = 8;
const ASM_LABEL_CAP: usize = 32;
const ASM_DATA_CAP: usize = 64;

fn assemble_program(lines: &[&str]) -> StdVec<Word> {
    let mut buffer = [0u16; 256];
    let builder =
        ProgramBuilder::<ASM_MACHINE_MAX, ASM_FUNCTION_MAX>::new(&mut buffer, 1).unwrap();
    let mut asm: Assembler<ASM_MACHINE_MAX, ASM_FUNCTION_MAX, ASM_LABEL_CAP, ASM_DATA_CAP> =
        Assembler::new(builder);
    for line in lines {
        asm.add_line(line).unwrap();
    }
    let descriptor = asm.finish().unwrap();
    buffer[..descriptor.length].to_vec()
}

fn run_single(
    program: &[Word],
    globals: &mut [Word],
    stack: &mut Vec<Word, STACK_CAP>,
) -> Result<(), MachineError> {
    let mut program = Program::new(program, globals)?;
    program.call(0, 0, stack)
}

#[test]
fn test_init_get_color() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 3 functions 2",
        ".func set_rgb index 0",
        "STORE 0",
        "STORE 1",
        "STORE 2",
        "EXIT",
        ".end",
        ".func get_rgb index 1",
        "LOAD 0",
        "LOAD 1",
        "LOAD 2",
        "EXIT",
        ".end",
        ".end",
    ]);

    println!("program {:?}", program);

    let mut globals = [0u16; 10];
    let (red, green, blue) = (17, 23, 31);
    let mut stack: Vec<Word, 100> = Vec::new();

    {
        let mut program = Program::new(program.as_slice(), globals.as_mut_slice())?;

        stack.push(red).unwrap();
        stack.push(green).unwrap();
        stack.push(blue).unwrap();
        program.init_machine(0, &mut stack)?;
    }
    assert_eq!(stack.len(), 0);

    println!("memory {:?}", globals);

    {
        let mut program = Program::new(program.as_slice(), globals.as_mut_slice())?;

        println!("stack is {:?}", stack);

        stack.push(0).unwrap();
        stack.push(0).unwrap();
        stack.push(0).unwrap();
        let (r, g, b) = program
            .get_led_color(0, 31337, 17, &mut stack)
            .expect("Could not get led color");

        println!("stack is {:?}", stack);
        assert_eq!((r as u16, g as u16, b as u16), (red, green, blue));
    }

    assert_eq!(stack.len(), 5);
    assert_eq!((stack[0], stack[1], stack[2]), (0, 0, 0));
    assert_eq!((stack[3], stack[4]), (31337, 17));
    Ok(())
}


#[test]
fn test_init_get_color2() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 3 functions 2",
        ".func set_rgb index 0",
        "STORE 0",
        "STORE 1",
        "STORE 2",
        "EXIT",
        ".end",
        ".func get_rgb index 1",
        "LOAD 0",
        "ADD",
        "PUSH 255",
        "MOD",
        "LOAD 1",
        "LOAD 2",
        "EXIT",
        ".end",
        ".end",
    ]);

    println!("program {:?}", program);

    let mut globals = [0u16; 10];
    let (red, green, blue) = (17, 23, 31);
    let mut stack: Vec<Word, 100> = Vec::new();

    {
        let mut program = Program::new(program.as_slice(), globals.as_mut_slice())?;

        stack.push(red).unwrap();
        stack.push(green).unwrap();
        stack.push(blue).unwrap();
        program.init_machine(0, &mut stack)?;
    }
    assert_eq!(stack.len(), 0);

    println!("memory {:?}", globals);

    {
        let mut program = Program::new(program.as_slice(), globals.as_mut_slice())?;

        println!("stack is {:?}", stack);

        stack.push(0).unwrap();
        stack.push(0).unwrap();
        stack.push(0).unwrap();
        let (r, g, b) = program
            .get_led_color(0, 31337, 1, &mut stack)
            .expect("Could not get led color");

        println!("stack is {:?}", stack);
        assert_eq!((r as u16, g as u16, b as u16), (red, green, blue + 1));

        assert_eq!(stack.len(), 4);
        assert_eq!((stack[0], stack[1], stack[2]), (0, 0, 0));
        assert_eq!(stack[3], 31337);
        stack.clear();

        stack.push(0).unwrap();
        stack.push(0).unwrap();
        stack.push(0).unwrap();
        let (r, g, b) = program
            .get_led_color(0, 31337, 30, &mut stack)
            .expect("Could not get led color");

        println!("stack is {:?}", stack);
        assert_eq!((r as u16, g as u16, b as u16), (red, green, blue + 30));
    }

    assert_eq!(stack.len(), 4);
    assert_eq!((stack[0], stack[1], stack[2]), (0, 0, 0));
    assert_eq!(stack[3], 31337);
    Ok(())
}


#[test]
fn op_push() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 42",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[42]);
    Ok(())
}

#[test]
fn op_pop() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "POP",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert!(stack.is_empty());
    Ok(())
}

#[test]
fn op_dup() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 5",
        "DUP",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[5, 5]);
    Ok(())
}

#[test]
fn op_swap() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "SWAP",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[2, 1]);
    Ok(())
}

#[test]
fn op_sload() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 10",
        "PUSH 20",
        "SLOAD 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[10, 20, 10]);
    Ok(())
}

#[test]
fn op_sload_named() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        ".frame first 0",
        ".frame second 1",
        "PUSH 10",
        "PUSH 20",
        "SLOAD first",
        "SLOAD second",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[10, 20, 10, 20]);
    Ok(())
}

#[test]
fn op_sstore() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "PUSH 3",
        "SSTORE 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[3, 2]);
    Ok(())
}

#[test]
fn op_sstore_named() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        ".frame replace 0",
        ".frame keep 1",
        "PUSH 1",
        "PUSH 2",
        "PUSH 3",
        "SSTORE replace",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[3, 2]);
    Ok(())
}

#[test]
fn op_load() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 1 functions 1",
        ".func main index 0",
        "LOAD 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [99u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[99]);
    Ok(())
}

#[test]
fn op_store() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 1 functions 1",
        ".func main index 0",
        "PUSH 7",
        "STORE 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(globals[0], 7);
    assert!(stack.is_empty());
    Ok(())
}

#[test]
fn op_load_named_global() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 2 functions 1",
        ".global red 0",
        ".global blue 1",
        ".func main index 0",
        "LOAD red",
        "LOAD blue",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [10u16, 20u16];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[10, 20]);
    Ok(())
}

#[test]
fn op_store_named_global() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 2 functions 1",
        ".global red 0",
        ".global blue 1",
        ".func main index 0",
        "PUSH 55",
        "STORE red",
        "PUSH 77",
        "STORE blue",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 2];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(globals, [55, 77]);
    assert!(stack.is_empty());
    Ok(())
}

#[test]
fn op_load_static() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".data consts",
        "consts:",
        ".word 123",
        ".end",
        ".func main index 0",
        "LOAD_STATIC consts",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[123]);
    Ok(())
}

#[test]
fn op_jump() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "JUMP target",
        "PUSH 1",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_less_than() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "BRLT target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_less_than_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 3",
        "PUSH 2",
        "BRLT target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_less_than_eq() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 2",
        "PUSH 2",
        "BRLTE target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_less_than_eq_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 2",
        "PUSH 1",
        "BRLTE target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_greater_than() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 3",
        "PUSH 2",
        "BRGT target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_greater_than_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 2",
        "PUSH 3",
        "BRGT target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_greater_than_eq() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 2",
        "PUSH 2",
        "BRGTE target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_greater_than_eq_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "BRGTE target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_equal() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 5",
        "PUSH 5",
        "BREQ target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_equal_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 5",
        "PUSH 6",
        "BREQ target",
        "PUSH 9",
        "EXIT",
        "target:",
        "PUSH 7",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_and() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "AND",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_and_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 0",
        "AND",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_or() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "PUSH 2",
        "OR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_or_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "PUSH 0",
        "OR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_xor() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "PUSH 2",
        "XOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_xor_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 1",
        "XOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_not() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "NOT",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_not_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "NOT",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_bitwise_and() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0x0F0F",
        "PUSH 0x00FF",
        "BAND",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0x000F]);
    Ok(())
}

#[test]
fn op_bitwise_or() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0x0F0F",
        "PUSH 0x00F0",
        "BOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0x0FFF]);
    Ok(())
}

#[test]
fn op_bitwise_xor() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0x0F0F",
        "PUSH 0x00FF",
        "BXOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0x0FF0]);
    Ok(())
}

#[test]
fn op_bitwise_not() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 0x00FF",
        "BNOT",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0xFF00]);
    Ok(())
}

#[test]
fn op_multiply() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 6",
        "PUSH 7",
        "MUL",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[42]);
    Ok(())
}

#[test]
fn op_divide() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 84",
        "PUSH 7",
        "DIV",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[12]);
    Ok(())
}

#[test]
fn op_mod() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 29",
        "PUSH 5",
        "MOD",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[4]);
    Ok(())
}

#[test]
fn op_add() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 5",
        "PUSH 7",
        "ADD",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[12]);
    Ok(())
}

#[test]
fn op_subtract() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 10",
        "PUSH 3",
        "SUB",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_exit() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "EXIT",
        "PUSH 2",
        ".end",
        ".end",
    ]);
    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_return() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main globals 0 functions 2",
        ".func helper index 1",
        "PUSH 66",
        "PUSH 77",
        "PUSH 88",
        "PUSH 99",
        "RET 3",
        ".end",
        ".func main index 0",
        "PUSH 0",
        "CALL helper",
        "EXIT",
        ".end",
        ".end",
    ]); 

    let mut globals = [0u16; 1];
    let mut stack: Vec<Word, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[77, 88, 99]);
    Ok(())
}
