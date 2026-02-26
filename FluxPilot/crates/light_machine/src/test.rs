use super::*;
use crate::assembler::Assembler;
use crate::builder::ProgramBuilder;

extern crate std;
use std::println;
use std::format;
use std::string::{String, ToString};
use std::vec::Vec as StdVec;
use std::vec;

const STACK_CAP: usize = 32;
const ASM_MACHINE_MAX: usize = 1;
const ASM_FUNCTION_MAX: usize = 8;
const ASM_LABEL_CAP: usize = 32;
const ASM_DATA_CAP: usize = 64;

fn assemble_program(lines: &[&str]) -> StdVec<ProgramWord> {
    let mut buffer = [0u16; 256];
    let builder = ProgramBuilder::<ASM_MACHINE_MAX, ASM_FUNCTION_MAX>::new(
        &mut buffer,
        1,
        1,
        0,
    )
    .unwrap();
    let mut asm: Assembler<ASM_MACHINE_MAX, ASM_FUNCTION_MAX, ASM_LABEL_CAP, ASM_DATA_CAP> =
        Assembler::new(builder);
    for line in lines {
        asm.add_line(line).unwrap();
    }
    let descriptor = asm.finish().unwrap();
    buffer[..descriptor.length].to_vec()
}

fn assemble_program_with_shared(
    lines: &[&str],
    machine_count: ProgramWord,
    shared_function_count: ProgramWord,
) -> StdVec<ProgramWord> {
    let mut buffer = [0u16; 256];
    let builder = ProgramBuilder::<ASM_MACHINE_MAX, ASM_FUNCTION_MAX>::new(
        &mut buffer,
        machine_count,
        machine_count,
        shared_function_count,
    )
    .unwrap();
    let mut asm: Assembler<ASM_MACHINE_MAX, ASM_FUNCTION_MAX, ASM_LABEL_CAP, ASM_DATA_CAP> =
        Assembler::new(builder);
    for line in lines {
        asm.add_line(line).unwrap();
    }
    let descriptor = asm.finish().unwrap();
    buffer[..descriptor.length].to_vec()
}

fn make_memory(program: &[ProgramWord], stack_capacity: usize) -> StdVec<StackWord> {
    let globals_size = program
        .get(GLOBALS_SIZE_OFFSET)
        .copied()
        .unwrap_or(0);
    let globals_len = usize::from(globals_size);
    let total_words = globals_len + stack_capacity;
    vec![0u32; total_words]
}

fn run_single(
    program: &[ProgramWord],
    globals: &mut [StackWord],
    stack: &mut Vec<StackWord, STACK_CAP>,
) -> Result<(), MachineError> {
    let mut memory = make_memory(program, STACK_CAP);
    let globals_len = globals.len();
    if globals_len > memory.len() {
        return Err(MachineError::GlobalsBufferTooSmall(
            ProgramWord::try_from(globals_len).unwrap_or(ProgramWord::MAX),
        ));
    }
    memory[..globals_len].copy_from_slice(globals);
    let mut program = Program::new(program, memory.as_mut_slice())?;
    {
        let stack_slice = program.stack_mut();
        stack_slice.clear();
        for value in stack.iter() {
            stack_slice.push(*value)?;
        }
    }
    program.call(0, 0)?;
    stack.clear();
    for value in program.stack().as_slice() {
        if stack.push(*value).is_err() {
            return Err(MachineError::StackOverflow);
        }
    }
    globals.copy_from_slice(&memory[..globals_len]);
    Ok(())
}

#[test]
fn test_shared_function_call() -> Result<(), MachineError> {
    let lines = [
        ".shared shared0 0",
        ".shared_func helper index 0",
        "    GLOAD shared0",
        "    RET 1",
        ".end",
        ".machine main locals 0 functions 1",
        "    .func main index 0",
        "        PUSH 0",
        "        CALL_SHARED helper",
        "        EXIT",
        "    .end",
        ".end",
    ];
    let program_words = assemble_program_with_shared(&lines, 1, 1);
    let mut memory = make_memory(&program_words, STACK_CAP);
    memory[0] = 42;
    let mut program = Program::new(&program_words, memory.as_mut_slice())?;
    program.call(0, 0)?;
    let value = program
        .stack_mut()
        .pop()
        .ok_or(MachineError::StackUnderFlow)?;
    assert_eq!(value, 42);
    Ok(())
}

#[test]
fn test_shared_function_index_out_of_range() -> Result<(), MachineError> {
    let lines = [
        ".shared_func helper index 0",
        "    RET 0",
        ".end",
        ".machine main locals 0 functions 1",
        "    .func main index 0",
        "        PUSH 0",
        "        CALL_SHARED 1",
        "        EXIT",
        "    .end",
        ".end",
    ];
    let program_words = assemble_program_with_shared(&lines, 1, 1);
    let mut memory = make_memory(&program_words, STACK_CAP);
    let mut program = Program::new(&program_words, memory.as_mut_slice())?;
    let err = program.call(0, 0).unwrap_err();
    assert!(matches!(err, MachineError::SharedFunctionIndexOutOfRange(_)));
    Ok(())
}

#[test]
fn test_shared_function_can_access_locals() -> Result<(), MachineError> {
    let lines = [
        ".shared_func helper index 0",
        "    LLOAD 0",
        "    RET 1",
        ".end",
        ".machine main locals 1 functions 1",
        "    .func main index 0",
        "        PUSH 99",
        "        LSTORE 0",
        "        PUSH 0",
        "        CALL_SHARED helper",
        "        EXIT",
        "    .end",
        ".end",
    ];
    let program_words = assemble_program_with_shared(&lines, 1, 1);
    let mut memory = make_memory(&program_words, STACK_CAP);
    let mut program = Program::new(&program_words, memory.as_mut_slice())?;
    program.call(0, 0)?;
    let value = program
        .stack_mut()
        .pop()
        .ok_or(MachineError::StackUnderFlow)?;
    assert_eq!(value, 99);
    Ok(())
}

#[test]
fn test_shared_function_call_underflow() -> Result<(), MachineError> {
    let lines = [
        ".shared_func helper index 0",
        "    RET 0",
        ".end",
        ".machine main locals 0 functions 1",
        "    .func main index 0",
        "        CALL_SHARED helper",
        "        EXIT",
        "    .end",
        ".end",
    ];
    let program_words = assemble_program_with_shared(&lines, 1, 1);
    let mut memory = make_memory(&program_words, STACK_CAP);
    let mut program = Program::new(&program_words, memory.as_mut_slice())?;
    let err = program.call(0, 0).unwrap_err();
    assert!(matches!(err, MachineError::StackUnderFlow));
    Ok(())
}

#[test]
fn test_invalid_program_version() {
    let program = [0u16, 0, 0, 0];
    let mut memory = make_memory(&program, STACK_CAP);
    let err = match Program::new(&program, memory.as_mut_slice()) {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert!(matches!(err, MachineError::InvalidProgramVersion(_)));
}

#[test]
fn test_memory_too_small_for_globals() {
    let program = assemble_program(&[
        ".machine main locals 3 functions 1",
        ".func main index 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut memory = vec![0u32; 2];
    let err = match Program::new(program.as_slice(), memory.as_mut_slice()) {
        Ok(_) => panic!("expected error"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        MachineError::MemoryBufferTooSmall { .. } | MachineError::GlobalsBufferTooSmall(_)
    ));
}

#[test]
fn test_min_stack_capacity_overflow() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut memory = make_memory(program.as_slice(), 1);
    let mut program = Program::new(program.as_slice(), memory.as_mut_slice())?;
    let err = program.call(0, 0).unwrap_err();
    assert!(matches!(err, MachineError::StackOverflow));
    Ok(())
}

fn build_simple_crawler_machine_lines(name: &str, init: [ProgramWord; 6]) -> StdVec<String> {
    let source = format!(
        "
.machine {} locals 7 functions 8
    .local red 0
    .local green 1
    .local blue 2
    .local speed 3
    .local brightness 4
    .local led_count 5
    .local frame_tick 6
    .data control_statics
    init_red:
    .word {}
    init_green:
    .word {}
    init_blue:
    .word {}
    init_speed:
    .word {}
    init_brightness:
    .word {}
    init_led_count:
    .word {}
    .end

    .func init index 0
        LOAD_STATIC init_red
        LSTORE red
        LOAD_STATIC init_green
        LSTORE green
        LOAD_STATIC init_blue
        LSTORE blue
        LOAD_STATIC init_speed
        LSTORE speed
        LOAD_STATIC init_brightness
        LSTORE brightness
        LOAD_STATIC init_led_count
        LSTORE led_count
        PUSH 0
        LSTORE frame_tick
        EXIT
    .end

    .func start_frame index 1
        LSTORE frame_tick
        EXIT
    .end

    .func set_rgb index 3
        LSTORE blue
        LSTORE green
        LSTORE red
        EXIT
    .end

    .func set_brightness index 4
        LSTORE brightness
        EXIT
    .end

    .func set_speed index 5
        LSTORE speed
        EXIT
    .end

    .func set_led_count index 7
        LSTORE led_count
        EXIT
    .end

    .func get_rgb_worker index 6
        .frame sred 0
        .frame sgreen 1
        .frame sblue 2
        .frame led_index 3
        .frame ticks 4
        SLOAD led_index
        SLOAD ticks
        LLOAD speed
        LLOAD led_count
        MUL
        MOD
        LLOAD speed
        DIV
        BREQ match
        SLOAD sred
        SLOAD sgreen
        SLOAD sblue
        RET 3
        match:
        LLOAD red
        LLOAD brightness
        MUL
        PUSH 100
        DIV
        LLOAD green
        LLOAD brightness
        MUL
        PUSH 100
        DIV
        LLOAD blue
        LLOAD brightness
        MUL
        PUSH 100
        DIV
        RET 3
    .end

    .func get_rgb index 2
        LLOAD frame_tick
        PUSH 5
        CALL get_rgb_worker
        EXIT
    .end
.end",
        name, init[0], init[1], init[2], init[3], init[4], init[5]
    );

    source.lines().map(|line| line.to_string()).collect()
}

#[test]
fn test_locals_are_machine_scoped() -> Result<(), MachineError> {
    let mut buffer = [0u16; 256];
    let builder = ProgramBuilder::<2, 1>::new(&mut buffer, 2, 2, 0).unwrap();
    let mut asm: Assembler<2, 1, 16, 16> = Assembler::new(builder);

    asm.add_line(".machine first locals 1 functions 1").unwrap();
    asm.add_line(".func set index 0").unwrap();
    asm.add_line("LSTORE 0").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    asm.add_line(".machine second locals 1 functions 1").unwrap();
    asm.add_line(".func set index 0").unwrap();
    asm.add_line("LSTORE 0").unwrap();
    asm.add_line("EXIT").unwrap();
    asm.add_line(".end").unwrap();
    asm.add_line(".end").unwrap();

    let descriptor = asm.finish().unwrap();
    let program = &buffer[..descriptor.length];
    let mut memory = make_memory(program, STACK_CAP);

    {
        let mut program = Program::new(program, memory.as_mut_slice())?;
        program.stack_mut().push(11)?;
        program.call(0, 0)?;
    }
    assert_eq!(&memory[..2], &[11, 0]);

    {
        let mut program = Program::new(program, memory.as_mut_slice())?;
        program.stack_mut().push(22)?;
        program.call(1, 0)?;
    }
    assert_eq!(&memory[..2], &[11, 22]);
    Ok(())
}

#[test]
fn test_four_simple_crawlers_in_one_program() -> Result<(), MachineError> {
    const MACHINE_COUNT: usize = 4;
    const FUNCTION_COUNT: usize = 8;
    const LABEL_CAP: usize = 32;
    const DATA_CAP: usize = 32;

    let mut buffer = [0u16; 768];
    let builder = ProgramBuilder::<MACHINE_COUNT, FUNCTION_COUNT>::new(
        &mut buffer,
        MACHINE_COUNT as ProgramWord,
        MACHINE_COUNT as ProgramWord,
        0,
    )
    .unwrap();
    let mut asm: Assembler<MACHINE_COUNT, FUNCTION_COUNT, LABEL_CAP, DATA_CAP> =
        Assembler::new(builder);

    let init_values: [[ProgramWord; 6]; MACHINE_COUNT] = [
        [10, 20, 30, 2, 100, 256],
        [40, 50, 60, 3, 80,  256],
        [70, 80, 90, 4, 60,  256],
        [15, 25, 35, 5, 90,  256],
    ];

    for (index, init) in init_values.iter().enumerate() {
        let name = format!("crawler{}", index + 1);
        let lines = build_simple_crawler_machine_lines(&name, *init);
        for line in lines.iter() {
            asm.add_line(line).unwrap();
        }
    }

    let descriptor = asm.finish().unwrap();

    println!("program length {}", descriptor.length);

    let program = &buffer[..descriptor.length];
    let mut memory = make_memory(program, STACK_CAP);
    let mut program = Program::new(program, memory.as_mut_slice())?;

    let machine_count = program.machine_count()?;

    assert_eq!(machine_count, MACHINE_COUNT as u16);

    for machine_index in 0..machine_count {
        program.init_machine(machine_index as ProgramWord)?;
        assert!(program.stack().is_empty());
    }

    for (machine_index, init) in init_values.iter().enumerate() {
        program.start_frame(machine_index as ProgramWord, 0)?;
        {
            let stack = program.stack_mut();
            stack.clear();
            stack.push(0)?;
            stack.push(0)?;
            stack.push(0)?;
        }
        let (r, g, b) =
            program.get_led_color(machine_index as ProgramWord, 0)?;
        let expected_r = (init[0] * init[4]) / 100;
        let expected_g = (init[1] * init[4]) / 100;
        let expected_b = (init[2] * init[4]) / 100;
        assert_eq!(
            (r, g, b),
            (expected_r as u8, expected_g as u8, expected_b as u8)
        );
    }

    for i in  8000..8100 {
        for machine_index in 0..machine_count {
            program.start_frame(machine_index as ProgramWord, i as u32)?;
        }
        for j in 0..256 {
            for machine_index in 0..machine_count {
                {
                    let stack = program.stack_mut();
                    stack.clear();
                    stack.push(0)?;
                    stack.push(0)?;
                    stack.push(0)?;
                }
                program.get_led_color(machine_index as ProgramWord, j as u16)?;
            
            }
        }
    }


    Ok(())
}

#[test]
fn test_init_get_color() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 3 functions 3",
        ".func set_rgb index 0",
        "LSTORE 2",
        "LSTORE 1",
        "LSTORE 0",
        "EXIT",
        ".end",
        ".func start_frame index 1",
        "POP",
        "EXIT",
        ".end",
        ".func get_rgb index 2",
        "LLOAD 0",
        "LLOAD 1",
        "LLOAD 2",
        "EXIT",
        ".end",
        ".end",
    ]);

    println!("program {:?}", program);

    let mut memory = make_memory(program.as_slice(), 100);
    let (red, green, blue): (u16, u16, u16) = (17, 23, 31);

    let mut program = Program::new(program.as_slice(), memory.as_mut_slice())?;
    {
        let stack = program.stack_mut();
        stack.push(StackWord::from(red))?;
        stack.push(StackWord::from(green))?;
        stack.push(StackWord::from(blue))?;
    }
    program.init_machine(0)?;
    assert_eq!(program.stack().len(), 0);

    println!("stack is {:?}", program.stack().as_slice());
    {
        let stack = program.stack_mut();
        stack.push(0)?;
        stack.push(0)?;
        stack.push(0)?;
    }
    let (r, g, b) = program
        .get_led_color(0, 31337)
        .expect("Could not get led color");

    println!("stack is {:?}", program.stack().as_slice());
    assert_eq!((r as u16, g as u16, b as u16), (red, green, blue));

    let stack = program.stack();
    assert_eq!(stack.len(), 4);
    assert_eq!((stack[0], stack[1], stack[2]), (0, 0, 0));
    assert_eq!(stack[3], 31337);
    Ok(())
}


#[test]
fn test_init_get_color2() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 4 functions 3",
        ".func set_rgb index 0",
        "LSTORE 2",
        "LSTORE 1",
        "LSTORE 0",
        "PUSH 0",
        "LSTORE 3",
        "EXIT",
        ".end",
        ".func start_frame index 1",
        "LSTORE 3",
        "EXIT",
        ".end",
        ".func get_rgb index 2",
        "LLOAD 0",
        "LLOAD 3",
        "ADD",
        "PUSH 255",
        "MOD",
        "LLOAD 1",
        "LLOAD 2",
        "EXIT",
        ".end",
        ".end",
    ]);

    println!("program {:?}", program);

    let mut memory = make_memory(program.as_slice(), 100);
    let (red, green, blue): (u16, u16, u16) = (17, 23, 31);

    let mut program = Program::new(program.as_slice(), memory.as_mut_slice())?;
    {
        let stack = program.stack_mut();
        stack.push(StackWord::from(red))?;
        stack.push(StackWord::from(green))?;
        stack.push(StackWord::from(blue))?;
    }
    program.init_machine(0)?;
    assert_eq!(program.stack().len(), 0);

    println!("stack is {:?}", program.stack().as_slice());
    {
        let stack = program.stack_mut();
        stack.push(0)?;
        stack.push(0)?;
        stack.push(0)?;
    }
    program.start_frame(0, 1u32)?;
    let (r, g, b) = program
        .get_led_color(0, 31337)
        .expect("Could not get led color");

    println!("stack is {:?}", program.stack().as_slice());
    assert_eq!((r as u16, g as u16, b as u16), (red + 1, green, blue));

    {
        let stack = program.stack();
        assert_eq!(stack.len(), 4);
        assert_eq!((stack[0], stack[1], stack[2]), (0, 0, 0));
        assert_eq!(stack[3], 31337);
    }
    {
        let stack = program.stack_mut();
        stack.clear();
        stack.push(0)?;
        stack.push(0)?;
        stack.push(0)?;
    }
    program.start_frame(0, 30u32)?;
    let (r, g, b) = program
        .get_led_color(0, 31337)
        .expect("Could not get led color");

    println!("stack is {:?}", program.stack().as_slice());
    assert_eq!((r as u16, g as u16, b as u16), (red + 30, green, blue));

    let stack = program.stack();
    assert_eq!(stack.len(), 4);
    assert_eq!((stack[0], stack[1], stack[2]), (0, 0, 0));
    assert_eq!(stack[3], 31337);
    Ok(())
}


#[test]
fn op_push() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 42",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[42]);
    Ok(())
}

#[test]
fn op_pop() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "POP",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert!(stack.is_empty());
    Ok(())
}

#[test]
fn op_dup() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 5",
        "DUP",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[5, 5]);
    Ok(())
}

#[test]
fn op_swap() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "SWAP",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[2, 1]);
    Ok(())
}

#[test]
fn op_sload() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 10",
        "PUSH 20",
        "SLOAD 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[10, 20, 10]);
    Ok(())
}

#[test]
fn op_sload_named() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[10, 20, 10, 20]);
    Ok(())
}

#[test]
fn op_sstore() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "PUSH 3",
        "SSTORE 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[3, 2]);
    Ok(())
}

#[test]
fn op_sstore_named() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[3, 2]);
    Ok(())
}

#[test]
fn op_lload() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 1 functions 1",
        ".func main index 0",
        "LLOAD 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [99u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[99]);
    Ok(())
}

#[test]
fn op_lstore() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 1 functions 1",
        ".func main index 0",
        "PUSH 7",
        "LSTORE 0",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(globals[0], 7);
    assert!(stack.is_empty());
    Ok(())
}

#[test]
fn op_lload_named_local() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 2 functions 1",
        ".local red 0",
        ".local blue 1",
        ".func main index 0",
        "LLOAD red",
        "LLOAD blue",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [10u32, 20u32];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[10, 20]);
    Ok(())
}

#[test]
fn op_lstore_named_local() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 2 functions 1",
        ".local red 0",
        ".local blue 1",
        ".func main index 0",
        "PUSH 55",
        "LSTORE red",
        "PUSH 77",
        "LSTORE blue",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 2];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(globals, [55, 77]);
    assert!(stack.is_empty());
    Ok(())
}

#[test]
fn op_load_static() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[123]);
    Ok(())
}

#[test]
fn op_jump() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_less_than() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_less_than_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_less_than_eq() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_less_than_eq_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_greater_than() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_greater_than_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_greater_than_eq() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_greater_than_eq_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_branch_equal() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_branch_equal_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
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
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[9]);
    Ok(())
}

#[test]
fn op_and() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 2",
        "AND",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_and_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 0",
        "AND",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_or() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "PUSH 2",
        "OR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_or_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "PUSH 0",
        "OR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_xor() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "PUSH 2",
        "XOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_xor_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "PUSH 1",
        "XOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_not() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0",
        "NOT",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_not_false() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "NOT",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0]);
    Ok(())
}

#[test]
fn op_bitwise_and() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0x0F0F",
        "PUSH 0x00FF",
        "BAND",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0x000F]);
    Ok(())
}

#[test]
fn op_bitwise_or() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0x0F0F",
        "PUSH 0x00F0",
        "BOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0x0FFF]);
    Ok(())
}

#[test]
fn op_bitwise_xor() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0x0F0F",
        "PUSH 0x00FF",
        "BXOR",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0x0FF0]);
    Ok(())
}

#[test]
fn op_bitwise_not() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 0x00FF",
        "BNOT",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[0xFFFF_FF00]);
    Ok(())
}

#[test]
fn op_multiply() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 6",
        "PUSH 7",
        "MUL",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[42]);
    Ok(())
}

#[test]
fn op_divide() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 84",
        "PUSH 7",
        "DIV",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[12]);
    Ok(())
}

#[test]
fn op_mod() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 29",
        "PUSH 5",
        "MOD",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[4]);
    Ok(())
}

#[test]
fn op_add() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 5",
        "PUSH 7",
        "ADD",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[12]);
    Ok(())
}

#[test]
fn op_subtract() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 10",
        "PUSH 3",
        "SUB",
        "EXIT",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[7]);
    Ok(())
}

#[test]
fn op_exit() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 1",
        ".func main index 0",
        "PUSH 1",
        "EXIT",
        "PUSH 2",
        ".end",
        ".end",
    ]);
    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[1]);
    Ok(())
}

#[test]
fn op_return() -> Result<(), MachineError> {
    let program = assemble_program(&[
        ".machine main locals 0 functions 2",
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

    let mut globals = [0u32; 1];
    let mut stack: Vec<StackWord, STACK_CAP> = Vec::new();
    run_single(&program, &mut globals, &mut stack)?;
    assert_eq!(stack.as_slice(), &[77, 88, 99]);
    Ok(())
}
