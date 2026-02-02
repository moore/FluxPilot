// This is a simple assembler. It is just a ruff
// draft and where it has been reviewed by me and I made changes it was
// designed and written by a LLM. I took this aproach becouse I really
// don't have strong feelings about assemblly and just wanted this out
// of the way so I could get on to other stuff I cared about.

use heapless::{String, Vec};

use crate::builder::{
    FunctionBuilder,
    FunctionIndex,
    MachineBuilder,
    MachineBuilderError,
    Op,
    ProgramBuilder,
    SharedFunctionBuilder,
};
use crate::ProgramWord;

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
    DuplicateGlobal,
    DuplicateStackSlot,
    GlobalIndexOutOfRange,
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
    offset: ProgramWord,
}

struct Fixup {
    name: String<NAME_CAP>,
    at: ProgramWord,
}

struct FuncEntry {
    name: String<NAME_CAP>,
    index: ProgramWord,
    defined: bool,
}

enum BlockKind {
    None,
    Machine,
    Function,
    Data,
    SharedFunction,
    SharedData,
}

pub struct Assembler<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize, const LABEL_CAP: usize, const DATA_CAP: usize> {
    program: Option<ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    machine: Option<MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    function: Option<FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    shared_function: Option<SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>>,
    block: BlockKind,
    labels: Vec<Label, LABEL_CAP>,
    static_labels: Vec<Label, LABEL_CAP>,
    fixups: Vec<Fixup, LABEL_CAP>,
    funcs: Vec<FuncEntry, FUNCTION_COUNT_MAX>,
    shared_funcs: Vec<FuncEntry, FUNCTION_COUNT_MAX>,
    globals: Vec<Label, LABEL_CAP>,
    shared_globals: Vec<Label, LABEL_CAP>,
    stack_slots: Vec<Label, LABEL_CAP>,
    data: Vec<ProgramWord, DATA_CAP>,
    cursor: ProgramWord,
    function_base: ProgramWord,
    next_function_index: ProgramWord,
    function_count: ProgramWord,
    shared_function_count: ProgramWord,
    next_shared_function_index: ProgramWord,
    globals_size: ProgramWord,
    globals_base: ProgramWord,
    shared_globals_size: ProgramWord,
    shared_globals_locked: bool,
    line_number: u32,
}

impl<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize, const LABEL_CAP: usize, const DATA_CAP: usize>
    Assembler<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX, LABEL_CAP, DATA_CAP>
{
    pub fn new(builder: ProgramBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>) -> Self {
        let shared_function_count = builder.shared_function_count();
        Self {
            program: Some(builder),
            machine: None,
            function: None,
            shared_function: None,
            block: BlockKind::None,
            labels: Vec::new(),
            static_labels: Vec::new(),
            fixups: Vec::new(),
            funcs: Vec::new(),
            shared_funcs: Vec::new(),
            globals: Vec::new(),
            shared_globals: Vec::new(),
            stack_slots: Vec::new(),
            data: Vec::new(),
            cursor: 0,
            function_base: 0,
            next_function_index: 0,
            function_count: 0,
            shared_function_count,
            next_shared_function_index: 0,
            globals_size: 0,
            globals_base: 0,
            shared_globals_size: 0,
            shared_globals_locked: false,
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

        if matches!(self.block, BlockKind::Data | BlockKind::SharedData) && first != ".end" {
            return self
                .handle_data_line(&tokens)
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
        for entry in self.shared_funcs.iter() {
            if !entry.defined {
                return Err(AssemblerError::Kind(
                    AssemblerErrorKind::FunctionNotDeclared,
                ));
            }
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
        let offset = match self.block {
            BlockKind::Function => self
                .function_base
                .checked_add(self.cursor)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?,
            BlockKind::Data => self.cursor,
            _ => self.cursor,
        };
        self.labels
            .push(Label { name, offset })
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
            ".shared_func" => self.start_shared_function(tokens),
            ".shared_func_decl" => self.declare_shared_function(tokens),
            ".local" => self.declare_local(tokens),
            ".shared" => self.declare_shared(tokens),
            ".frame" => self.declare_stack_slot(tokens),
            ".data" => self.start_data(tokens),
            ".shared_data" => self.start_shared_data(tokens),
            ".end" => self.end_block(),
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective)),
        }
    }

    fn start_machine(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        // Strict token shape keeps the grammar unambiguous.
        let locals_token = tokens.get(2).copied();
        let functions_token = tokens.get(4).copied();
        if tokens.len() != 6
            || !matches!(locals_token, Some("locals") | Some("globals"))
            || functions_token != Some("functions")
        {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let globals_size = parse_word(tokens.get(3).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        let function_count = parse_word(tokens.get(5).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        self.labels.clear();
        self.static_labels.clear();
        self.fixups.clear();
        self.globals.clear();
        self.cursor = 0;
        self.function_count = function_count;
        self.next_function_index = 0;
        self.funcs.clear();
        self.globals_size = globals_size;
        let program = self
            .program
            .take()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingProgram))?;
        let mut program = program;
        if !self.shared_globals_locked {
            program.set_shared_globals_size(self.shared_globals_size)?;
            self.shared_globals_locked = true;
        }
        let machine = program.new_machine(function_count, globals_size)?;
        self.globals_base = machine.globals_offset();
        self.machine = Some(machine);
        self.block = BlockKind::Machine;
        Ok(())
    }

    fn start_shared_function(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
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
        } else if let Some(entry) = self.shared_funcs.iter().find(|entry| entry.name == name) {
            entry.index
        } else {
            self.next_free_shared_function_index()?
        };

        if index >= self.shared_function_count {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::FunctionIndexOutOfRange,
            ));
        }
        self.mark_shared_function_defined(&name, index)?;

        self.labels.clear();
        self.fixups.clear();
        self.stack_slots.clear();
        self.cursor = 0;
        let program = self
            .program
            .take()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingProgram))?;
        let mut program = program;
        if !self.shared_globals_locked {
            program.set_shared_globals_size(self.shared_globals_size)?;
            self.shared_globals_locked = true;
        }
        let function = program.new_shared_function_at_index(FunctionIndex::new(index))?;
        self.function_base = function.function_start();
        self.shared_function = Some(function);
        self.block = BlockKind::SharedFunction;
        Ok(())
    }

    fn declare_local(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        if tokens.len() != 3 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let name = to_name(tokens.get(1).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        if self.globals.iter().any(|entry| entry.name == name) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateGlobal));
        }
        if self.shared_globals.iter().any(|entry| entry.name == name) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateGlobal));
        }
        let index = parse_word(tokens.get(2).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        if index >= self.globals_size {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::GlobalIndexOutOfRange,
            ));
        }
        self.globals
            .push(Label { name, offset: index })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
        Ok(())
    }

    fn declare_shared(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) || self.shared_globals_locked {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        if tokens.len() != 3 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let name = to_name(tokens.get(1).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        if self.shared_globals.iter().any(|entry| entry.name == name) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateGlobal));
        }
        let index = parse_word(tokens.get(2).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        let next_size = index
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::GlobalIndexOutOfRange))?;
        if next_size > self.shared_globals_size {
            self.shared_globals_size = next_size;
        }
        self.shared_globals
            .push(Label { name, offset: index })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
        Ok(())
    }

    fn declare_stack_slot(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Function) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        if tokens.len() != 3 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        let name = to_name(tokens.get(1).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        if self.stack_slots.iter().any(|entry| entry.name == name) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateStackSlot));
        }
        let offset = parse_word(tokens.get(2).ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?)?;
        self.stack_slots
            .push(Label { name, offset })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
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
        self.fixups.clear();
        self.stack_slots.clear();
        self.cursor = 0;
        let machine = self
            .machine
            .take()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingMachine))?;
        let function = machine.new_function_at_index(FunctionIndex::new(index))?;
        self.function_base = function.function_start();
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

    fn declare_shared_function(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
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
            self.next_free_shared_function_index()?
        };

        if index >= self.shared_function_count {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::FunctionIndexOutOfRange,
            ));
        }
        if self.shared_funcs.iter().any(|entry| entry.index == index) {
            return Err(AssemblerError::Kind(
                AssemblerErrorKind::FunctionIndexDuplicate,
            ));
        }
        self.shared_funcs
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

    fn start_shared_data(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        if tokens.len() != 2 {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        }
        self.labels.clear();
        self.data.clear();
        self.cursor = 0;
        self.block = BlockKind::SharedData;
        Ok(())
    }

    fn end_block(&mut self) -> Result<(), AssemblerError> {
        match self.block {
            BlockKind::Function => {
                let function = self
                    .function
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                let function = self.resolve_fixups(function)?;
                let (_index, machine) = function.finish()?;
                self.machine = Some(machine);
                self.block = BlockKind::Machine;
                Ok(())
            }
            BlockKind::SharedFunction => {
                let function = self
                    .shared_function
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                let function = self.resolve_shared_fixups(function)?;
                let (_index, program) = function.finish()?;
                self.program = Some(program);
                self.block = BlockKind::None;
                Ok(())
            }
            BlockKind::Data => {
                let machine = self
                    .machine
                    .as_mut()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingMachine))?;
                let data_base = machine.program_free();
                for label in self.labels.iter() {
                    let absolute = data_base
                        .checked_add(label.offset)
                        .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                    self.static_labels
                        .push(Label {
                            name: label.name.clone(),
                            offset: absolute,
                        })
                        .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
                }
                if !self.data.is_empty() {
                    let _ = machine.add_static(self.data.as_slice())?;
                }
                self.data.clear();
                self.labels.clear();
                self.block = BlockKind::Machine;
                Ok(())
            }
            BlockKind::SharedData => {
                let program = self
                    .program
                    .as_mut()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingProgram))?;
                let data_base = program.program_free();
                for label in self.labels.iter() {
                    let absolute = data_base
                        .checked_add(label.offset)
                        .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                    self.static_labels
                        .push(Label {
                            name: label.name.clone(),
                            offset: absolute,
                        })
                        .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
                }
                if !self.data.is_empty() {
                    let _ = program.add_shared_static(self.data.as_slice())?;
                }
                self.data.clear();
                self.labels.clear();
                self.block = BlockKind::None;
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
            BlockKind::SharedFunction => self.handle_function_instruction(tokens),
            BlockKind::SharedData => self.handle_data_line(tokens),
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
        let mnemonic = tokens.first().copied().ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidInstruction,
        ))?;
        match mnemonic {
            "LOAD_STATIC" | "load_static" => self.emit_stack_target(tokens, Op::LoadStatic),
            "JUMP" | "jump" => self.emit_stack_target(tokens, Op::Jump),
            "CALL" | "call" => self.emit_stack_target(tokens, Op::Call),
            "CALL_SHARED" | "call_shared" => self.emit_shared_stack_target(tokens, Op::CallShared),
            "BRLT" | "brlt" => self.emit_stack_target(tokens, Op::BranchLessThan),
            "BRLTE" | "brlte" => self.emit_stack_target(tokens, Op::BranchLessThanEq),
            "BRGT" | "brgt" => self.emit_stack_target(tokens, Op::BranchGreaterThan),
            "BRGTE" | "brgte" => self.emit_stack_target(tokens, Op::BranchGreaterThanEq),
            "BREQ" | "breq" => self.emit_stack_target(tokens, Op::BranchEqual),
            _ => {
                let (op, width) = self.parse_op(tokens)?;
                if width == 0 {
                    return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction));
                }
                self.cursor = self
                    .cursor
                    .checked_add(width)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                match self.block {
                    BlockKind::Function => {
                        let function = self
                            .function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(op)?;
                    }
                    BlockKind::SharedFunction => {
                        let function = self
                            .shared_function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(op)?;
                    }
                    _ => {
                        return Err(AssemblerError::Kind(
                            AssemblerErrorKind::UnexpectedInstruction,
                        ))
                    }
                }
                Ok(())
            }
        }
    }

    fn emit_stack_target(&mut self, tokens: &[&str], op: Op) -> Result<(), AssemblerError> {
        match tokens.len() {
            1 => {
                self.cursor = self
                    .cursor
                    .checked_add(1)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                match self.block {
                    BlockKind::Function => {
                        let function = self
                            .function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(op)?;
                    }
                    BlockKind::SharedFunction => {
                        let function = self
                            .shared_function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(op)?;
                    }
                    _ => return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedInstruction)),
                }
                Ok(())
            }
            2 => {
                let operand_token = tokens.get(1).copied().ok_or(AssemblerError::Kind(
                    AssemblerErrorKind::InvalidInstruction,
                ))?;
                let operand = self
                    .resolve_operand(operand_token)?
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction))?;
                self.cursor = self
                    .cursor
                    .checked_add(3)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                match self.block {
                    BlockKind::Function => {
                        let function = self
                            .function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(Op::Push(operand))?;
                        function.add_op(op)?;
                    }
                    BlockKind::SharedFunction => {
                        let function = self
                            .shared_function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(Op::Push(operand))?;
                        function.add_op(op)?;
                    }
                    _ => return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedInstruction)),
                }
                Ok(())
            }
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
        }
    }

    fn emit_shared_stack_target(&mut self, tokens: &[&str], op: Op) -> Result<(), AssemblerError> {
        match tokens.len() {
            1 => {
                self.cursor = self
                    .cursor
                    .checked_add(1)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                match self.block {
                    BlockKind::Function => {
                        let function = self
                            .function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(op)?;
                    }
                    BlockKind::SharedFunction => {
                        let function = self
                            .shared_function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(op)?;
                    }
                    _ => return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedInstruction)),
                }
                Ok(())
            }
            2 => {
                let operand_token = tokens.get(1).copied().ok_or(AssemblerError::Kind(
                    AssemblerErrorKind::InvalidInstruction,
                ))?;
                let operand = self
                    .resolve_shared_function_operand(operand_token)?
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction))?;
                self.cursor = self
                    .cursor
                    .checked_add(3)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                match self.block {
                    BlockKind::Function => {
                        let function = self
                            .function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(Op::Push(operand))?;
                        function.add_op(op)?;
                    }
                    BlockKind::SharedFunction => {
                        let function = self
                            .shared_function
                            .as_mut()
                            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                        function.add_op(Op::Push(operand))?;
                        function.add_op(op)?;
                    }
                    _ => return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedInstruction)),
                }
                Ok(())
            }
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
        }
    }

    fn next_free_function_index(&mut self) -> Result<ProgramWord, AssemblerError> {
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

    fn next_free_shared_function_index(&mut self) -> Result<ProgramWord, AssemblerError> {
        while self
            .shared_funcs
            .iter()
            .any(|entry| entry.index == self.next_shared_function_index)
        {
            self.next_shared_function_index = self
                .next_shared_function_index
                .checked_add(1)
                .ok_or(AssemblerError::Kind(
                    AssemblerErrorKind::FunctionIndexOutOfRange,
                ))?;
        }
        let index = self.next_shared_function_index;
        self.next_shared_function_index = self
            .next_shared_function_index
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange))?;
        Ok(index)
    }

    fn mark_function_defined(&mut self, name: &String<NAME_CAP>, index: ProgramWord) -> Result<(), AssemblerError> {
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

    fn mark_shared_function_defined(
        &mut self,
        name: &String<NAME_CAP>,
        index: ProgramWord,
    ) -> Result<(), AssemblerError> {
        if let Some(entry) = self
            .shared_funcs
            .iter_mut()
            .find(|entry| entry.name == *name)
        {
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
        self.shared_funcs
            .push(FuncEntry {
                name: name.clone(),
                index,
                defined: true,
            })
            .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxFunctionsExceeded))
    }

    fn resolve_fixups(
        &mut self,
        mut function: FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    ) -> Result<FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, AssemblerError> {
        while let Some(fixup) = self.fixups.pop() {
            let label = self
                .labels
                .iter()
                .find(|label| label.name == fixup.name)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))?;
            function.patch_word(fixup.at, label.offset)?;
        }
        Ok(function)
    }

    fn resolve_shared_fixups(
        &mut self,
        mut function: SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    ) -> Result<SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, AssemblerError> {
        while let Some(fixup) = self.fixups.pop() {
            let label = self
                .labels
                .iter()
                .find(|label| label.name == fixup.name)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))?;
            function.patch_word(fixup.at, label.offset)?;
        }
        Ok(function)
    }

    fn parse_op(&mut self, tokens: &[&str]) -> Result<(Op, ProgramWord), AssemblerError> {
        let mnemonic = tokens.first().copied().ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidInstruction,
        ))?;
        let operand_token = tokens.get(1).copied();
        let expects_operand = matches!(
            mnemonic,
            "PUSH"
                | "push"
                | "LLOAD"
                | "lload"
                | "LSTORE"
                | "lstore"
                | "GLOAD"
                | "gload"
                | "GSTORE"
                | "gstore"
                | "SLOAD"
                | "sload"
                | "SSTORE"
                | "sstore"
                | "RET"
                | "ret"
        );

        let operand = if expects_operand {
            let token = operand_token.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?;
            if matches!(mnemonic, "SLOAD" | "sload" | "SSTORE" | "sstore") {
                self.resolve_stack_operand(token)?
            } else if matches!(mnemonic, "LLOAD" | "lload" | "LSTORE" | "lstore") {
                self.resolve_local_operand(token)?
            } else if matches!(mnemonic, "GLOAD" | "gload" | "GSTORE" | "gstore") {
                self.resolve_shared_global_operand(token)?
            } else {
                self.resolve_operand(token)?
            }
        } else {
            None
        };

        let op = match mnemonic {
            "PUSH" | "push" => Op::Push(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "POP" | "pop" => Op::Pop,
            "LLOAD" | "lload" => Op::LocalLoad(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "LSTORE" | "lstore" => Op::LocalStore(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "GLOAD" | "gload" => Op::GlobalLoad(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "GSTORE" | "gstore" => Op::GlobalStore(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "SLOAD" | "sload" => Op::StackLoad(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "SSTORE" | "sstore" => Op::StackStore(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "DUP" | "dup" => Op::Dup,
            "SWAP" | "swap" => Op::Swap,
            "RET" | "ret" => Op::Return(operand.ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidInstruction,
            ))?),
            "LOAD_STATIC" | "load_static" => Op::LoadStatic,
            "JUMP" | "jump" => Op::Jump,
            "CALL" | "call" => Op::Call,
            "CALL_SHARED" | "call_shared" => Op::CallShared,
            "BRLT" | "brlt" => Op::BranchLessThan,
            "BRLTE" | "brlte" => Op::BranchLessThanEq,
            "BRGT" | "brgt" => Op::BranchGreaterThan,
            "BRGTE" | "brgte" => Op::BranchGreaterThanEq,
            "BREQ" | "breq" => Op::BranchEqual,
            "EXIT" | "exit" => Op::Exit,
            "AND" | "and" => Op::And,
            "OR" | "or" => Op::Or,
            "XOR" | "xor" => Op::Xor,
            "NOT" | "not" => Op::Not,
            "BAND" | "band" => Op::BitwiseAnd,
            "BOR" | "bor" => Op::BitwiseOr,
            "BXOR" | "bxor" => Op::BitwiseXor,
            "BNOT" | "bnot" => Op::BitwiseNot,
            "ADD" | "add" => Op::Add,
            "SUB" | "sub" => Op::Subtract,
            "MUL" | "mul" => Op::Multiply,
            "DIV" | "div" => Op::Devide,
            "MOD" | "mod" => Op::Mod,
            _ => return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
        };

        let width = if expects_operand { 2 } else { 1 };
        Ok((op, width))
    }

    fn resolve_operand(&mut self, token: &str) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            return Ok(Some(value));
        }

        let name = to_name(token)?;
        if let Some(label) = self.labels.iter().find(|label| label.name == name) {
            return Ok(Some(label.offset));
        }
        if let Some(label) = self.static_labels.iter().find(|label| label.name == name) {
            return Ok(Some(label.offset));
        }
        if let Some(entry) = self.funcs.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.index));
        }
        if let Some(entry) = self.globals.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.offset));
        }
        if let Some(entry) = self.shared_globals.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.offset));
        }

        if matches!(self.block, BlockKind::Function) {
            let at = self
                .function_base
                .checked_add(self.cursor)
                .and_then(|base| base.checked_add(1))
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
            self.fixups
                .push(Fixup { name, at })
                .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::MaxLabelsExceeded))?;
            return Ok(Some(0));
        }

        Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
    }

    fn resolve_shared_function_operand(
        &mut self,
        token: &str,
    ) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            return Ok(Some(value));
        }
        let name = to_name(token)?;
        if let Some(entry) = self.shared_funcs.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.index));
        }
        Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
    }

    fn resolve_stack_operand(&mut self, token: &str) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            return Ok(Some(value));
        }
        let name = to_name(token)?;
        if let Some(entry) = self.stack_slots.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.offset));
        }
        self.resolve_operand(token)
    }

    fn resolve_local_operand(&mut self, token: &str) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            if matches!(self.block, BlockKind::SharedFunction) {
                return Ok(Some(value));
            }
            if value >= self.globals_size {
                return Err(AssemblerError::Kind(
                    AssemblerErrorKind::GlobalIndexOutOfRange,
                ));
            }
            return Ok(Some(value));
        }
        let name = to_name(token)?;
        if let Some(entry) = self.globals.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.offset));
        }
        Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
    }

    fn resolve_shared_global_operand(
        &mut self,
        token: &str,
    ) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            if value >= self.shared_globals_size {
                return Err(AssemblerError::Kind(
                    AssemblerErrorKind::GlobalIndexOutOfRange,
                ));
            }
            return Ok(Some(value));
        }
        let name = to_name(token)?;
        if let Some(entry) = self.shared_globals.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.offset));
        }
        Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
    }
}

fn parse_word(token: &str) -> Result<ProgramWord, AssemblerError> {
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
