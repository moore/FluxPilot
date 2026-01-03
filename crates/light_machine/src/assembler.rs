// This is a simple assembler. It is just a ruff
// draft and where it has been reviewed by me and I made changes it was
// designed and written by a LLM. I took this aproach becouse I really
// don't have strong feelings about assemblly and just wanted this out
// of the way so I could get on to other stuff I cared about.

use heapless::{String, Vec};

use crate::builder::{FunctionBuilder, FunctionIndex, MachineBuilder, MachineBuilderError, Op, ProgramBuilder};
use crate::Word;

const MAX_TOKENS: usize = 6;
const NAME_CAP: usize = 32;

#[derive(Debug)]
pub enum AssemblerError {
    Kind(AssemblerErrorKind),
    WithLine { line: u32, kind: AssemblerErrorKind },
}

impl AssemblerError {
    fn with_line(self, line: u32) -> Self {
        match self {
            AssemblerError::WithLine { .. } => self,
            AssemblerError::Kind(kind) => AssemblerError::WithLine { line, kind },
        }
    }

    pub fn line_number(&self) -> Option<u32> {
        match self {
            Self::Kind(_) => None,
            Self::WithLine { line, .. } => Some(*line)
        }
    }

    pub fn error_kind(&self) -> &AssemblerErrorKind {
        match self {
            Self::Kind(kind) => kind,
            Self::WithLine { kind, .. } => kind,
        }
    }
}

#[derive(Debug)]
pub enum AssemblerErrorKind {
    EmptyLine,
    TooManyTokens,
    InvalidDirective,
    InvalidInstruction,
    InvalidNumber,
    NameTooLong,
    DuplicateLabel,
    MaxLabelsExceeded,
    UnknownLabel,
    MissingMachine,
    MissingFunction,
    MissingProgram,
    UnexpectedDirective,
    UnexpectedInstruction,
    FunctionAlreadyDefined,
    FunctionNotDeclared,
    FunctionIndexOutOfRange,
    FunctionIndexDuplicate,
    MaxFunctionsExceeded,
    LineNumberOverflow,
    CursorOverflow,
    DataTooLarge,
    Builder(MachineBuilderError),
}

impl From<MachineBuilderError> for AssemblerError {
    fn from(err: MachineBuilderError) -> Self {
        AssemblerError::Kind(AssemblerErrorKind::Builder(err))
    }
}

#[derive(Clone)]
struct Label {
    name: String<NAME_CAP>,
    offset: Word,
}

struct FuncEntry {
    name: String<NAME_CAP>,
    index: Word,
    defined: bool,
}

enum BlockKind {
    None,
    Machine,
    Function,
    Data,
}

pub struct Assembler<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize, const LABEL_CAP: usize, const DATA_CAP: usize> {
    program: Option<ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    machine: Option<MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    function: Option<FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    block: BlockKind,
    labels: Vec<Label, LABEL_CAP>,
    funcs: Vec<FuncEntry, FUNCTION_COUNT_MAX>,
    data: Vec<Word, DATA_CAP>,
    cursor: Word,
    next_function_index: Word,
    function_count: Word,
    line_number: u32,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize, const LABEL_CAP: usize, const DATA_CAP: usize>
    Assembler<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX, LABEL_CAP, DATA_CAP>
{
    pub fn new(builder: ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>) -> Self {
        Self {
            program: Some(builder),
            machine: None,
            function: None,
            block: BlockKind::None,
            labels: Vec::new(),
            funcs: Vec::new(),
            data: Vec::new(),
            cursor: 0,
            next_function_index: 0,
            function_count: 0,
            line_number: 0,
        }
    }

    pub fn add_line(&mut self, line: &str) -> Result<(), AssemblerError> {
        self.line_number = self
            .line_number
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::LineNumberOverflow))?;
        let line_number = self.line_number;
        let line = strip_comment(line);
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }

        // Token limit keeps parsing bounded in no_std/heapless mode.
        let mut tokens: Vec<&str, MAX_TOKENS> = Vec::new();
        for token in line.split_whitespace() {
            tokens.push(token).map_err(|_| {
                AssemblerError::Kind(AssemblerErrorKind::TooManyTokens).with_line(line_number)
            })?;
        }
        if tokens.is_empty() {
            return Err(AssemblerError::Kind(AssemblerErrorKind::EmptyLine).with_line(line_number));
        }

        let first = match tokens.first() {
            Some(token) => *token,
            None => {
                return Err(AssemblerError::Kind(AssemblerErrorKind::EmptyLine).with_line(line_number));
            }
        };

        // Labels must be a single token ending with ':' to keep parsing one-pass.
        if tokens.len() == 1 && first.ends_with(':') {
            return self
                .add_label(first)
                .map_err(|err| err.with_line(line_number));
        }

        // Directives always start with '.' to avoid ambiguity with mnemonics.
        if first.starts_with('.') {
            return self
                .handle_directive(&tokens)
                .map_err(|err| err.with_line(line_number));
        }

        self.handle_instruction(&tokens)
            .map_err(|err| err.with_line(line_number))
    }


    pub fn finish(mut self) -> Result<crate::ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, AssemblerError> {
        match self.block {
            BlockKind::None => {}
            _ => return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective)),
        }
        let program = self
            .program
            .take()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingProgram))?;
        Ok(program.finish_program())
    }

    fn add_label(&mut self, token: &str) -> Result<(), AssemblerError> {
        let name = token.trim_end_matches(':');
        let name = to_name(name)?;
        if self.labels.iter().any(|label| label.name == name) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateLabel));
        }
        self.labels
            .push(Label {
                name,
                offset: self.cursor,
            })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
        Ok(())
    }

    fn handle_directive(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        let Some(token) = tokens.first() else {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        };
        match *token {
            ".machine" => self.start_machine(tokens),
            ".func" => self.start_function(tokens),
            ".func_decl" => self.declare_function(tokens),
            ".data" => self.start_data(tokens),
            ".end" => self.end_block(),
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective)),
        }
    }

    fn start_machine(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        // Strict token shape keeps the grammar unambiguous.
        let globals_token = tokens.get(2).copied();
        let functions_token = tokens.get(4).copied();
        if tokens.len() != 6 || globals_token != Some("globals") || functions_token != Some("functions") {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let globals_size = parse_word(tokens.get(3).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        let function_count = parse_word(tokens.get(5).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        self.function_count = function_count;
        self.next_function_index = 0;
        self.funcs.clear();
        let program = self
            .program
            .take()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingProgram))?;
        let machine = program.new_machine(function_count, globals_size)?;
        self.machine = Some(machine);
        self.block = BlockKind::Machine;
        Ok(())
    }

    fn start_function(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        // Allow optional `index <I>`; any other shape is rejected.
        if tokens.len() != 2 && tokens.len() != 4 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let name = to_name(tokens.get(1).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        let index = if tokens.len() == 4 {
            if tokens.get(2).copied() != Some("index") {
                return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
            }
            parse_word(tokens.get(3).ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidDirective,
            ))?)?
        } else if let Some(entry) = self.funcs.iter().find(|entry| entry.name == name) {
            entry.index
        } else {
            self.next_free_function_index()?
        };

        if index >= self.function_count {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::FunctionIndexOutOfRange,
            ));
        }
        self.mark_function_defined(&name, index)?;

        self.labels.clear();
        self.cursor = 0;
        let machine = self
            .machine
            .take()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingMachine))?;
        let function = machine.new_function_at_index(FunctionIndex::new(index))?;
        self.function = Some(function);
        self.block = BlockKind::Function;
        Ok(())
    }

    fn declare_function(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        // Allow optional `index <I>`; any other shape is rejected.
        if tokens.len() != 2 && tokens.len() != 4 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let name = to_name(tokens.get(1).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        let index = if tokens.len() == 4 {
            if tokens.get(2).copied() != Some("index") {
                return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
            }
            parse_word(tokens.get(3).ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidDirective,
            ))?)?
        } else {
            self.next_free_function_index()?
        };

        if index >= self.function_count {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::FunctionIndexOutOfRange,
            ));
        }
        if self.funcs.iter().any(|entry| entry.index == index) {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::FunctionIndexDuplicate,
            ));
        }
        self.funcs
            .push(FuncEntry {
                name,
                index,
                defined: false,
            })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxFunctionsExceeded))?;
        Ok(())
    }

    fn start_data(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        // `.data <name>` is the only accepted form.
        if tokens.len() != 2 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        self.labels.clear();
        self.data.clear();
        self.cursor = 0;
        self.block = BlockKind::Data;
        Ok(())
    }

    fn end_block(&mut self) -> Result<(), AssemblerError> {
        match self.block {
            BlockKind::Function => {
                let function = self
                    .function
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                let (_index, machine) = function.finish()?;
                self.machine = Some(machine);
                self.block = BlockKind::Machine;
                Ok(())
            }
            BlockKind::Data => {
                let machine = self
                    .machine
                    .as_mut()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingMachine))?;
                if !self.data.is_empty() {
                    let _ = machine.add_static(self.data.as_slice())?;
                }
                self.data.clear();
                self.block = BlockKind::Machine;
                Ok(())
            }
            BlockKind::Machine => {
                for entry in self.funcs.iter() {
                    if !entry.defined {
                        return Err(AssemblerError::Kind(
                            AssemblerErrorKind::FunctionNotDeclared,
                        ));
                    }
                }
                let machine = self
                    .machine
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingMachine))?;
                let program = machine.finish()?;
                self.program = Some(program);
                self.block = BlockKind::None;
                Ok(())
            }
            BlockKind::None => Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective)),
        }
    }

    fn handle_instruction(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        match self.block {
            BlockKind::Function => self.handle_function_instruction(tokens),
            BlockKind::Data => self.handle_data_line(tokens),
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedInstruction)),
        }
    }

    fn handle_data_line(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        // Data blocks allow labels for future LOAD_STATIC references.
        let first = tokens.first().copied().ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?;
        if tokens.len() == 1 && first.ends_with(':') {
            return self.add_label(first);
        }
        // Only allow `.word <num>` or a bare `<num>` to keep parsing simple.
        let value = if tokens.len() == 2 && first == ".word" {
            parse_word(tokens.get(1).ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidDirective,
            ))?)?
        } else if tokens.len() == 1 {
            parse_word(first)?
        } else {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        };
        self.data
            .push(value)
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::DataTooLarge))?;
        self.cursor = self
            .cursor
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
        Ok(())
    }

    fn handle_function_instruction(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        let function = self
            .function
            .as_mut()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
        let op = parse_op(tokens, &self.labels)?;
        self.cursor = self
            .cursor
            .checked_add(op_word_len(&op))
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
        function.add_op(op)?;
        Ok(())
    }

    fn next_free_function_index(&mut self) -> Result<Word, AssemblerError> {
        while self.funcs.iter().any(|entry| entry.index == self.next_function_index) {
            self.next_function_index = self
                .next_function_index
                .checked_add(1)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange))?;
        }
        let index = self.next_function_index;
        self.next_function_index = self
            .next_function_index
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange))?;
        Ok(index)
    }

    fn mark_function_defined(&mut self, name: &String<NAME_CAP>, index: Word) -> Result<(), AssemblerError> {
        if let Some(entry) = self.funcs.iter_mut().find(|entry| entry.name == *name) {
            if entry.defined {
                return Err(AssemblerError::Kind(
                    AssemblerErrorKind::FunctionAlreadyDefined,
                ));
            }
            entry.defined = true;
            if entry.index != index {
                return Err(AssemblerError::Kind(
                    AssemblerErrorKind::FunctionIndexDuplicate,
                ));
            }
            return Ok(());
        }
        self.funcs
            .push(FuncEntry {
                name: name.clone(),
                index,
                defined: true,
            })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxFunctionsExceeded))
    }
}



fn parse_op<const LABEL_CAP: usize>(tokens: &[&str], labels: &Vec<Label, LABEL_CAP>) -> Result<Op, AssemblerError> {
    let mnemonic = tokens.first().copied().ok_or(AssemblerError::Kind(
        AssemblerErrorKind::InvalidInstruction,
    ))?;
    let operand = if tokens.len() > 1 {
        Some(parse_operand(tokens.get(1).copied().ok_or(
            AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction),
        )?, labels)?)
    } else {
        None
    };

    match mnemonic {
        "PUSH" | "push" => Ok(Op::Push(require_operand(operand)?)),
        "POP" | "pop" => Ok(Op::Pop),
        "LOAD" | "load" => Ok(Op::Load(require_operand(operand)?)),
        "STORE" | "store" => Ok(Op::Store(require_operand(operand)?)),
        "LOAD_STATIC" | "load_static" => Ok(Op::LoadStatic(require_operand(operand)?)),
        "JUMP" | "jump" => Ok(Op::Jump(require_operand(operand)?)),
        "BRLT" | "brlt" => Ok(Op::BranchLessThan(require_operand(operand)?)),
        "BRLTE" | "brlte" => Ok(Op::BranchLessThanEq(require_operand(operand)?)),
        "BRGT" | "brgt" => Ok(Op::BranchGreaterThan(require_operand(operand)?)),
        "BRGTE" | "brgte" => Ok(Op::BranchGreaterThanEq(require_operand(operand)?)),
        "BREQ" | "breq" => Ok(Op::BranchEqual(require_operand(operand)?)),
        "RETURN" | "return" => Ok(Op::Return),
        "AND" | "and" => Ok(Op::And),
        "OR" | "or" => Ok(Op::Or),
        "XOR" | "xor" => Ok(Op::Xor),
        "NOT" | "not" => Ok(Op::Not),
        "BAND" | "band" => Ok(Op::BitwiseAnd),
        "BOR" | "bor" => Ok(Op::BitwiseOr),
        "BXOR" | "bxor" => Ok(Op::BitwiseXor),
        "BNOT" | "bnot" => Ok(Op::BitwiseNot),
        "ADD" | "add" => Ok(Op::Add),
        "SUB" | "sub" => Ok(Op::Subtract),
        "MUL" | "mul" => Ok(Op::Multiply),
        "DIV" | "div" => Ok(Op::Devide),
        _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
    }
}

fn parse_operand<const LABEL_CAP: usize>(token: &str, labels: &Vec<Label, LABEL_CAP>) -> Result<Word, AssemblerError> {
    if let Ok(value) = parse_word(token) {
        return Ok(value);
    }
    let name = to_name(token)?;
    for label in labels.iter() {
        if label.name == name {
            return Ok(label.offset);
        }
    }
    Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
}

fn require_operand(value: Option<Word>) -> Result<Word, AssemblerError> {
    value.ok_or(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction))
}

fn parse_word(token: &str) -> Result<Word, AssemblerError> {
    if let Some(hex) = token.strip_prefix("0x") {
        u16::from_str_radix(hex, 16)
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::InvalidNumber))
    } else {
        token
            .parse::<u16>()
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::InvalidNumber))
    }
}

fn strip_comment(line: &str) -> &str {
    match line.split(';').next() {
        Some(part) => part,
        None => line,
    }
}

fn to_name(name: &str) -> Result<String<NAME_CAP>, AssemblerError> {
    // Cap name length to keep identifiers bounded in heapless storage.
    let mut out: String<NAME_CAP> = String::new();
    out.push_str(name)
        .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::NameTooLong))?;
    Ok(out)
}

fn op_word_len(op: &Op) -> Word {
    match op {
        Op::Push(_)
        | Op::Load(_)
        | Op::Store(_)
        | Op::LoadStatic(_)
        | Op::Jump(_)
        | Op::BranchLessThan(_)
        | Op::BranchLessThanEq(_)
        | Op::BranchGreaterThan(_)
        | Op::BranchGreaterThanEq(_)
        | Op::BranchEqual(_) => 2,
        _ => 1,
    }
}
