use std::collections::HashMap;

use light_machine::assembler::{AssemblerError, AssemblerErrorKind};
use light_machine::{Ops, ProgramWord};

use crate::program_graph::{
    FunctionRef,
    ProgramGraph,
    ProgramGraphBuilder,
    SharedStaticId,
    StaticId,
    WordRef,
};

const MAX_TOKENS: usize = 6;
const NAME_CAP: usize = 32;

#[derive(Clone)]
struct Label {
    name: String,
    offset: ProgramWord,
}

struct Fixup {
    name: String,
    at: usize,
}

struct FuncEntry {
    name: String,
    index: ProgramWord,
    defined: bool,
}

struct StaticLabelRef {
    id: usize,
    offset: ProgramWord,
    shared: bool,
}

enum BlockKind {
    None,
    Machine,
    Function,
    Data,
    SharedFunction,
    SharedData,
}

struct FunctionAssembly {
    words: Vec<WordRef>,
}

enum OperandRef {
    Literal(ProgramWord),
    Label(String),
    Static(StaticLabelRef),
}

pub struct GraphAssembler {
    graph: ProgramGraphBuilder,
    block: BlockKind,
    labels: Vec<Label>,
    static_labels: HashMap<String, StaticLabelRef>,
    fixups: Vec<Fixup>,
    funcs: Vec<FuncEntry>,
    shared_funcs: Vec<FuncEntry>,
    globals: Vec<Label>,
    shared_globals: Vec<Label>,
    stack_slots: Vec<Label>,
    data: Vec<ProgramWord>,
    cursor: ProgramWord,
    function_count: ProgramWord,
    next_function_index: ProgramWord,
    shared_function_count: ProgramWord,
    next_shared_function_index: ProgramWord,
    globals_size: ProgramWord,
    shared_globals_size: ProgramWord,
    shared_globals_locked: bool,
    current_machine_statics: Vec<StaticId>,
    current_functions: Vec<FunctionRef>,
    current_function: Option<FunctionAssembly>,
    current_function_index: Option<ProgramWord>,
    current_shared_function_index: Option<ProgramWord>,
    line_number: u32,
}

impl GraphAssembler {
    pub fn new(shared_function_count: ProgramWord) -> Self {
        Self {
            graph: ProgramGraphBuilder::new(shared_function_count),
            block: BlockKind::None,
            labels: Vec::new(),
            static_labels: HashMap::new(),
            fixups: Vec::new(),
            funcs: Vec::new(),
            shared_funcs: Vec::new(),
            globals: Vec::new(),
            shared_globals: Vec::new(),
            stack_slots: Vec::new(),
            data: Vec::new(),
            cursor: 0,
            function_count: 0,
            next_function_index: 0,
            shared_function_count,
            next_shared_function_index: 0,
            globals_size: 0,
            shared_globals_size: 0,
            shared_globals_locked: false,
            current_machine_statics: Vec::new(),
            current_functions: Vec::new(),
            current_function: None,
            current_function_index: None,
            current_shared_function_index: None,
            line_number: 0,
        }
    }

    pub fn add_line(&mut self, line: &str) -> Result<(), AssemblerError> {
        self.line_number = self
            .line_number
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::LineNumberOverflow))?;
        let line_number = self.line_number;
        let line = strip_comment(line).trim();
        if line.is_empty() {
            return Ok(());
        }

        let mut tokens: Vec<&str> = Vec::new();
        for token in line.split_whitespace() {
            if tokens.len() >= MAX_TOKENS {
                return Err(AssemblerError::Kind(AssemblerErrorKind::TooManyTokens).with_line(line_number));
            }
            tokens.push(token);
        }
        if tokens.is_empty() {
            return Err(AssemblerError::Kind(AssemblerErrorKind::EmptyLine).with_line(line_number));
        }

        let first = *tokens
            .first()
            .ok_or_else(|| AssemblerError::Kind(AssemblerErrorKind::EmptyLine))?;

        if tokens.len() == 1 && first.ends_with(':') {
            return self.add_label(first).map_err(|err| err.with_line(line_number));
        }

        if matches!(self.block, BlockKind::Data | BlockKind::SharedData) && first != ".end" {
            return self.handle_data_line(&tokens).map_err(|err| err.with_line(line_number));
        }

        if first.starts_with('.') {
            return self
                .handle_directive(&tokens)
                .map_err(|err| err.with_line(line_number));
        }

        self.handle_instruction(&tokens)
            .map_err(|err| err.with_line(line_number))
    }

    pub fn finish(self) -> Result<ProgramGraph, AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
        for entry in &self.shared_funcs {
            if !entry.defined {
                return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionNotDeclared));
            }
        }
        Ok(self.graph.build())
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

    fn handle_instruction(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        match self.block {
            BlockKind::Function | BlockKind::SharedFunction => self.handle_function_instruction(tokens),
            BlockKind::Data | BlockKind::SharedData => self.handle_data_line(tokens),
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedInstruction)),
        }
    }

    fn start_machine(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::None) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
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
        self.static_labels.retain(|_, label| label.shared);
        self.fixups.clear();
        self.globals.clear();
        self.cursor = 0;
        self.function_count = function_count;
        self.next_function_index = 0;
        self.funcs.clear();
        self.globals_size = globals_size;
        self.current_machine_statics.clear();
        self.current_functions.clear();
        self.current_function_index = None;
        self.current_shared_function_index = None;
        if !self.shared_globals_locked {
            self.graph.set_shared_globals_size(self.shared_globals_size);
            self.shared_globals_locked = true;
        }
        self.block = BlockKind::Machine;
        Ok(())
    }

    fn start_function(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
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
        } else if let Some(entry) = self.funcs.iter().find(|entry| entry.name == name) {
            entry.index
        } else {
            self.next_free_function_index()?
        };

        if index >= self.function_count {
            return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange));
        }
        self.mark_function_defined(&name, index)?;

        self.labels.clear();
        self.fixups.clear();
        self.stack_slots.clear();
        self.cursor = 0;
        self.current_function = Some(FunctionAssembly { words: Vec::new() });
        self.current_function_index = Some(index);
        self.block = BlockKind::Function;
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
            return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange));
        }
        self.mark_shared_function_defined(&name, index)?;

        self.labels.clear();
        self.fixups.clear();
        self.stack_slots.clear();
        self.cursor = 0;
        if !self.shared_globals_locked {
            self.graph.set_shared_globals_size(self.shared_globals_size);
            self.shared_globals_locked = true;
        }
        self.current_function = Some(FunctionAssembly { words: Vec::new() });
        self.current_shared_function_index = Some(index);
        self.block = BlockKind::SharedFunction;
        Ok(())
    }

    fn declare_function(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
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
            self.next_free_function_index()?
        };

        if index >= self.function_count {
            return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange));
        }
        if self.funcs.iter().any(|entry| entry.index == index) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexDuplicate));
        }
        self.funcs.push(FuncEntry {
            name,
            index,
            defined: false,
        });
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
            return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange));
        }
        if self.shared_funcs.iter().any(|entry| entry.index == index) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexDuplicate));
        }
        self.shared_funcs.push(FuncEntry {
            name,
            index,
            defined: false,
        });
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
            return Err(AssemblerError::Kind(AssemblerErrorKind::GlobalIndexOutOfRange));
        }
        self.globals.push(Label { name, offset: index });
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
        self.shared_globals.push(Label { name, offset: index });
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
        self.stack_slots.push(Label { name, offset });
        Ok(())
    }

    fn start_data(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        if !matches!(self.block, BlockKind::Machine) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective));
        }
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
                let mut function = self
                    .current_function
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                self.resolve_fixups(&mut function)?;
                let function_id = self.graph.add_function(function.words);
                let index = self
                    .current_function_index
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                self.current_functions.push(FunctionRef {
                    index,
                    function_id,
                });
                self.block = BlockKind::Machine;
                Ok(())
            }
            BlockKind::SharedFunction => {
                let mut function = self
                    .current_function
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                self.resolve_fixups(&mut function)?;
                let index = self
                    .current_shared_function_index
                    .take()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                self.graph
                    .add_shared_function(index, function.words)
                    .map_err(|_| AssemblerError::Kind(AssemblerErrorKind::Builder(
                        light_machine::builder::MachineBuilderError::FunctionCoutExceeded,
                    )))?;
                self.block = BlockKind::None;
                Ok(())
            }
            BlockKind::Data => {
                let static_id = self.graph.add_static(&self.data);
                for label in &self.labels {
                    if self.static_labels.contains_key(&label.name) {
                        return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateLabel));
                    }
                    self.static_labels.insert(
                        label.name.clone(),
                        StaticLabelRef {
                            id: static_id.index(),
                            offset: label.offset,
                            shared: false,
                        },
                    );
                }
                self.current_machine_statics.push(static_id);
                self.data.clear();
                self.labels.clear();
                self.block = BlockKind::Machine;
                Ok(())
            }
            BlockKind::SharedData => {
                let shared_id = self.graph.add_shared_static(&self.data);
                for label in &self.labels {
                    if self.static_labels.contains_key(&label.name) {
                        return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateLabel));
                    }
                    self.static_labels.insert(
                        label.name.clone(),
                        StaticLabelRef {
                            id: shared_id.index(),
                            offset: label.offset,
                            shared: true,
                        },
                    );
                }
                self.data.clear();
                self.labels.clear();
                self.block = BlockKind::None;
                Ok(())
            }
            BlockKind::Machine => {
                for entry in &self.funcs {
                    if !entry.defined {
                        return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionNotDeclared));
                    }
                }
                let mut functions = self.current_functions.clone();
                functions.sort_by_key(|func| func.index);
                let type_id = self.graph.add_machine_type(
                    functions,
                    self.current_machine_statics.clone(),
                    self.globals_size,
                    self.function_count,
                );
                self.graph.add_machine_instance(type_id);
                self.block = BlockKind::None;
                Ok(())
            }
            BlockKind::None => Err(AssemblerError::Kind(AssemblerErrorKind::UnexpectedDirective)),
        }
    }

    fn handle_data_line(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
        let first = tokens.first().copied().ok_or(AssemblerError::Kind(
            AssemblerErrorKind::InvalidDirective,
        ))?;
        if tokens.len() == 1 && first.ends_with(':') {
            return self.add_label(first);
        }
        let value = if tokens.len() == 2 && first == ".word" {
            parse_word(tokens.get(1).ok_or(AssemblerError::Kind(
                AssemblerErrorKind::InvalidDirective,
            ))?)?
        } else if tokens.len() == 1 {
            parse_word(first)?
        } else {
            return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidDirective));
        };
        self.data.push(value);
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
            "LOAD_STATIC" | "load_static" => self.emit_stack_target(tokens, Ops::LoadStatic),
            "JUMP" | "jump" => self.emit_stack_target(tokens, Ops::Jump),
            "CALL" | "call" => self.emit_stack_target(tokens, Ops::Call),
            "CALL_SHARED" | "call_shared" => self.emit_shared_stack_target(tokens, Ops::CallShared),
            "BRLT" | "brlt" => self.emit_stack_target(tokens, Ops::BranchLessThan),
            "BRLTE" | "brlte" => self.emit_stack_target(tokens, Ops::BranchLessThanEq),
            "BRGT" | "brgt" => self.emit_stack_target(tokens, Ops::BranchGreaterThan),
            "BRGTE" | "brgte" => self.emit_stack_target(tokens, Ops::BranchGreaterThanEq),
            "BREQ" | "breq" => self.emit_stack_target(tokens, Ops::BranchEqual),
            _ => self.emit_simple_op(tokens),
        }
    }

    fn emit_simple_op(&mut self, tokens: &[&str]) -> Result<(), AssemblerError> {
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
                OperandRef::Literal(
                    self.resolve_stack_operand(token)?
                        .ok_or(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction))?,
                )
            } else if matches!(mnemonic, "LLOAD" | "lload" | "LSTORE" | "lstore") {
                OperandRef::Literal(
                    self.resolve_local_operand(token)?
                        .ok_or(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction))?,
                )
            } else if matches!(mnemonic, "GLOAD" | "gload" | "GSTORE" | "gstore") {
                OperandRef::Literal(
                    self.resolve_shared_global_operand(token)?
                        .ok_or(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction))?,
                )
            } else {
                self.resolve_operand(token)?
            }
        } else {
            OperandRef::Literal(0)
        };

        let opcode = match mnemonic {
            "PUSH" | "push" => Ops::Push,
            "POP" | "pop" => Ops::Pop,
            "LLOAD" | "lload" => Ops::LocalLoad,
            "LSTORE" | "lstore" => Ops::LocalStore,
            "GLOAD" | "gload" => Ops::GlobalLoad,
            "GSTORE" | "gstore" => Ops::GlobalStore,
            "SLOAD" | "sload" => Ops::StackLoad,
            "SSTORE" | "sstore" => Ops::StackStore,
            "DUP" | "dup" => Ops::Dup,
            "SWAP" | "swap" => Ops::Swap,
            "RET" | "ret" => Ops::Return,
            "EXIT" | "exit" => Ops::Exit,
            "AND" | "and" => Ops::And,
            "OR" | "or" => Ops::Or,
            "XOR" | "xor" => Ops::Xor,
            "NOT" | "not" => Ops::Not,
            "BAND" | "band" => Ops::BitwiseAnd,
            "BOR" | "bor" => Ops::BitwiseOr,
            "BXOR" | "bxor" => Ops::BitwiseXor,
            "BNOT" | "bnot" => Ops::BitwiseNot,
            "ADD" | "add" => Ops::Add,
            "SUB" | "sub" => Ops::Subtract,
            "MUL" | "mul" => Ops::Multiply,
            "DIV" | "div" => Ops::Divide,
            "MOD" | "mod" => Ops::Mod,
            _ => return Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
        };

        if expects_operand {
            self.cursor = self
                .cursor
                .checked_add(2)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
            self.push_word(WordRef::Literal(opcode.into()))?;
            self.push_operand(operand)?;
        } else {
            self.cursor = self
                .cursor
                .checked_add(1)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
            self.push_word(WordRef::Literal(opcode.into()))?;
        }
        Ok(())
    }

    fn emit_stack_target(&mut self, tokens: &[&str], opcode: Ops) -> Result<(), AssemblerError> {
        match tokens.len() {
            1 => {
                self.cursor = self
                    .cursor
                    .checked_add(1)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                self.push_word(WordRef::Literal(opcode.into()))?;
                Ok(())
            }
            2 => {
                let operand_token = tokens.get(1).copied().ok_or(AssemblerError::Kind(
                    AssemblerErrorKind::InvalidInstruction,
                ))?;
                let operand = self.resolve_operand(operand_token)?;
                self.cursor = self
                    .cursor
                    .checked_add(3)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                self.push_word(WordRef::Literal(Ops::Push.into()))?;
                self.push_operand(operand)?;
                self.push_word(WordRef::Literal(opcode.into()))?;
                Ok(())
            }
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
        }
    }

    fn emit_shared_stack_target(&mut self, tokens: &[&str], opcode: Ops) -> Result<(), AssemblerError> {
        match tokens.len() {
            1 => {
                self.cursor = self
                    .cursor
                    .checked_add(1)
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::CursorOverflow))?;
                self.push_word(WordRef::Literal(opcode.into()))?;
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
                self.push_word(WordRef::Literal(Ops::Push.into()))?;
                self.push_word(WordRef::Literal(operand))?;
                self.push_word(WordRef::Literal(opcode.into()))?;
                Ok(())
            }
            _ => Err(AssemblerError::Kind(AssemblerErrorKind::InvalidInstruction)),
        }
    }

    fn push_word(&mut self, word: WordRef) -> Result<(), AssemblerError> {
        let function = self
            .current_function
            .as_mut()
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
        function.words.push(word);
        Ok(())
    }

    fn push_operand(&mut self, operand: OperandRef) -> Result<(), AssemblerError> {
        match operand {
            OperandRef::Literal(value) => self.push_word(WordRef::Literal(value)),
            OperandRef::Static(label) => {
                if label.shared {
                    self.push_word(WordRef::SharedStatic(SharedStaticId::new(label.id), label.offset))
                } else {
                    self.push_word(WordRef::Static(StaticId::new(label.id), label.offset))
                }
            }
            OperandRef::Label(name) => {
                let function = self
                    .current_function
                    .as_ref()
                    .ok_or(AssemblerError::Kind(AssemblerErrorKind::MissingFunction))?;
                let at = function.words.len();
                self.fixups.push(Fixup { name, at });
                self.push_word(WordRef::LabelOffset(0))
            }
        }
    }

    fn resolve_fixups(&mut self, function: &mut FunctionAssembly) -> Result<(), AssemblerError> {
        while let Some(fixup) = self.fixups.pop() {
            let label = self
                .labels
                .iter()
                .find(|label| label.name == fixup.name)
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))?;
            let Some(slot) = function.words.get_mut(fixup.at) else {
                return Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel));
            };
            *slot = WordRef::LabelOffset(label.offset);
        }
        Ok(())
    }

    fn add_label(&mut self, token: &str) -> Result<(), AssemblerError> {
        let name = token.trim_end_matches(':');
        let name = to_name(name)?;
        if self.labels.iter().any(|label| label.name == name) {
            return Err(AssemblerError::Kind(AssemblerErrorKind::DuplicateLabel));
        }
        let offset = self.cursor;
        self.labels.push(Label { name, offset });
        Ok(())
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
                .ok_or(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange))?;
        }
        let index = self.next_shared_function_index;
        self.next_shared_function_index = self
            .next_shared_function_index
            .checked_add(1)
            .ok_or(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexOutOfRange))?;
        Ok(index)
    }

    fn mark_function_defined(&mut self, name: &str, index: ProgramWord) -> Result<(), AssemblerError> {
        if let Some(entry) = self.funcs.iter_mut().find(|entry| entry.name == name) {
            if entry.defined {
                return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionAlreadyDefined));
            }
            entry.defined = true;
            if entry.index != index {
                return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexDuplicate));
            }
            return Ok(());
        }
        self.funcs.push(FuncEntry {
            name: name.to_string(),
            index,
            defined: true,
        });
        Ok(())
    }

    fn mark_shared_function_defined(
        &mut self,
        name: &str,
        index: ProgramWord,
    ) -> Result<(), AssemblerError> {
        if let Some(entry) = self.shared_funcs.iter_mut().find(|entry| entry.name == name) {
            if entry.defined {
                return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionAlreadyDefined));
            }
            entry.defined = true;
            if entry.index != index {
                return Err(AssemblerError::Kind(AssemblerErrorKind::FunctionIndexDuplicate));
            }
            return Ok(());
        }
        self.shared_funcs.push(FuncEntry {
            name: name.to_string(),
            index,
            defined: true,
        });
        Ok(())
    }

    fn resolve_operand(&mut self, token: &str) -> Result<OperandRef, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            return Ok(OperandRef::Literal(value));
        }
        let name = to_name(token)?;
        if let Some(label) = self.labels.iter().find(|label| label.name == name) {
            return Ok(OperandRef::Literal(label.offset));
        }
        if let Some(label) = self.static_labels.get(&name) {
            return Ok(OperandRef::Static(StaticLabelRef {
                id: label.id,
                offset: label.offset,
                shared: label.shared,
            }));
        }
        if let Some(entry) = self.funcs.iter().find(|entry| entry.name == name) {
            return Ok(OperandRef::Literal(entry.index));
        }
        if let Some(entry) = self.globals.iter().find(|entry| entry.name == name) {
            return Ok(OperandRef::Literal(entry.offset));
        }
        if let Some(entry) = self.shared_globals.iter().find(|entry| entry.name == name) {
            return Ok(OperandRef::Literal(entry.offset));
        }
        if matches!(self.block, BlockKind::Function | BlockKind::SharedFunction) {
            return Ok(OperandRef::Label(name));
        }
        Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
    }

    fn resolve_shared_function_operand(&mut self, token: &str) -> Result<Option<ProgramWord>, AssemblerError> {
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
        self.resolve_operand(token).map(|operand| match operand {
            OperandRef::Literal(value) => Some(value),
            _ => None,
        })
    }

    fn resolve_local_operand(&mut self, token: &str) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            if matches!(self.block, BlockKind::SharedFunction) {
                return Ok(Some(value));
            }
            if value >= self.globals_size {
                return Err(AssemblerError::Kind(AssemblerErrorKind::GlobalIndexOutOfRange));
            }
            return Ok(Some(value));
        }
        let name = to_name(token)?;
        if let Some(entry) = self.globals.iter().find(|entry| entry.name == name) {
            return Ok(Some(entry.offset));
        }
        Err(AssemblerError::Kind(AssemblerErrorKind::UnknownLabel))
    }

    fn resolve_shared_global_operand(&mut self, token: &str) -> Result<Option<ProgramWord>, AssemblerError> {
        if let Ok(value) = parse_word(token) {
            if value >= self.shared_globals_size {
                return Err(AssemblerError::Kind(AssemblerErrorKind::GlobalIndexOutOfRange));
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

trait AssemblerErrorExt {
    fn with_line(self, line: u32) -> AssemblerError;
}

impl AssemblerErrorExt for AssemblerError {
    fn with_line(self, line: u32) -> AssemblerError {
        match self {
            AssemblerError::WithLine { .. } => self,
            AssemblerError::Kind(kind) => AssemblerError::WithLine { line, kind },
        }
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

fn to_name(name: &str) -> Result<String, AssemblerError> {
    if name.len() > NAME_CAP {
        return Err(AssemblerError::Kind(AssemblerErrorKind::NameTooLong));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod test {
    use super::*;
    use light_machine::builder::ProgramBuilder;

    fn compile_graph(source: &str) -> Result<ProgramGraph, AssemblerError> {
        let shared_function_count = source
            .lines()
            .filter(|line| line.trim_start().starts_with(".shared_func"))
            .count() as ProgramWord;
        let mut assembler = GraphAssembler::new(shared_function_count);
        for line in source.lines() {
            assembler.add_line(line)?;
        }
        assembler.finish()
    }

    #[test]
    fn graph_assembler_dedupes_identical_types() {
        let source = r#"
            .machine alpha locals 0 functions 1
            .func init index 0
            EXIT
            .end
            .end

            .machine beta locals 0 functions 1
            .func init index 0
            EXIT
            .end
            .end
        "#;
        let graph = compile_graph(source).unwrap();
        assert_eq!(graph.instance_count(), 2);
        assert_eq!(graph.type_count(), 1);
    }

    #[test]
    fn graph_assembler_resolves_forward_labels() {
        let source = r#"
            .machine alpha locals 0 functions 1
            .func init index 0
            JUMP target
            EXIT
            target:
            EXIT
            .end
            .end
        "#;
        let graph = compile_graph(source).unwrap();
        let mut buffer = [0u16; 128];
        let builder = ProgramBuilder::<2, 2>::new(
            &mut buffer,
            graph.instance_count(),
            graph.type_count(),
            graph.shared_function_count(),
        )
        .unwrap();
        let _descriptor = graph.emit_into(builder).unwrap();
    }

    #[test]
    fn graph_assembler_supports_shared_data_labels() {
        let source = r#"
            .shared_data config
            value:
            .word 42
            .end

            .machine alpha locals 0 functions 1
            .func init index 0
            LOAD_STATIC value
            EXIT
            .end
            .end
        "#;
        let graph = compile_graph(source).unwrap();
        let mut buffer = [0u16; 128];
        let builder = ProgramBuilder::<2, 2>::new(
            &mut buffer,
            graph.instance_count(),
            graph.type_count(),
            graph.shared_function_count(),
        )
        .unwrap();
        let _descriptor = graph.emit_into(builder).unwrap();
    }

    #[test]
    fn graph_assembler_supports_shared_function_calls() {
        let source = r#"
            .shared_func helper index 0
            EXIT
            .end

            .machine alpha locals 0 functions 1
            .func init index 0
            CALL_SHARED helper
            EXIT
            .end
            .end
        "#;
        let graph = compile_graph(source).unwrap();
        let mut buffer = [0u16; 128];
        let builder = ProgramBuilder::<2, 2>::new(
            &mut buffer,
            graph.instance_count(),
            graph.type_count(),
            graph.shared_function_count(),
        )
        .unwrap();
        let _descriptor = graph.emit_into(builder).unwrap();
    }
}
