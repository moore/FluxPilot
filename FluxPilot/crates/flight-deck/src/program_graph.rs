use std::collections::HashMap;

use light_machine::builder::{FunctionIndex, MachineBuilderError, Op, ProgramBuilder};
use light_machine::{ProgramDescriptor, ProgramWord};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StaticId(usize);

impl StaticId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SharedStaticId(usize);

impl SharedStaticId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FunctionId(usize);

impl FunctionId {
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MachineTypeId(usize);

impl MachineTypeId {
    pub fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum WordRef {
    Literal(ProgramWord),
    LabelOffset(ProgramWord),
    Static(StaticId, ProgramWord),
    SharedStatic(SharedStaticId, ProgramWord),
}

#[derive(Clone, Debug)]
pub struct FunctionNode {
    words: Vec<WordRef>,
}

#[derive(Clone, Debug)]
struct StaticDataNode {
    words: Vec<ProgramWord>,
}

#[derive(Clone, Debug)]
pub struct FunctionRef {
    pub index: ProgramWord,
    pub function_id: FunctionId,
}

#[derive(Clone, Debug)]
struct MachineTypeNode {
    functions: Vec<FunctionRef>,
    #[allow(dead_code)]
    statics: Vec<StaticId>,
    globals_size: ProgramWord,
    function_count: ProgramWord,
}

#[derive(Clone, Debug)]
struct MachineInstanceNode {
    type_id: MachineTypeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct MachineTypeKey {
    functions: Vec<(ProgramWord, FunctionId)>,
    statics: Vec<StaticId>,
    globals_size: ProgramWord,
    function_count: ProgramWord,
}

struct NodeInterner<K, V> {
    map: HashMap<K, usize>,
    nodes: Vec<V>,
}

impl<K, V> NodeInterner<K, V>
where
    K: Eq + std::hash::Hash,
{
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            nodes: Vec::new(),
        }
    }

    fn intern(&mut self, key: K, node: V) -> usize {
        if let Some(existing) = self.map.get(&key) {
            return *existing;
        }
        let id = self.nodes.len();
        self.nodes.push(node);
        self.map.insert(key, id);
        id
    }
}

pub struct ProgramGraphBuilder {
    shared_globals_size: ProgramWord,
    static_data: NodeInterner<Vec<ProgramWord>, StaticDataNode>,
    shared_static_data: NodeInterner<Vec<ProgramWord>, StaticDataNode>,
    functions: NodeInterner<Vec<WordRef>, FunctionNode>,
    types: NodeInterner<MachineTypeKey, MachineTypeNode>,
    instances: Vec<MachineInstanceNode>,
    shared_functions: HashMap<ProgramWord, FunctionNode>,
    shared_function_count: ProgramWord,
}

impl ProgramGraphBuilder {
    pub fn new(shared_function_count: ProgramWord) -> Self {
        Self {
            shared_globals_size: 0,
            static_data: NodeInterner::new(),
            shared_static_data: NodeInterner::new(),
            functions: NodeInterner::new(),
            types: NodeInterner::new(),
            instances: Vec::new(),
            shared_functions: HashMap::new(),
            shared_function_count,
        }
    }

    pub fn set_shared_globals_size(&mut self, size: ProgramWord) {
        self.shared_globals_size = size;
    }

    pub fn add_static(&mut self, data: &[ProgramWord]) -> StaticId {
        let key = data.to_vec();
        let id = self.static_data.intern(
            key.clone(),
            StaticDataNode { words: key },
        );
        StaticId(id)
    }

    pub fn add_shared_static(&mut self, data: &[ProgramWord]) -> SharedStaticId {
        let key = data.to_vec();
        let id = self.shared_static_data.intern(
            key.clone(),
            StaticDataNode { words: key },
        );
        SharedStaticId(id)
    }

    pub fn add_function(&mut self, words: Vec<WordRef>) -> FunctionId {
        let id = self
            .functions
            .intern(words.clone(), FunctionNode { words });
        FunctionId(id)
    }

    pub fn add_shared_function(
        &mut self,
        index: ProgramWord,
        words: Vec<WordRef>,
    ) -> Result<(), MachineBuilderError> {
        if index >= self.shared_function_count {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        if self.shared_functions.contains_key(&index) {
            return Err(MachineBuilderError::FunctionCoutExceeded);
        }
        self.shared_functions.insert(index, FunctionNode { words });
        Ok(())
    }

    pub fn add_machine_type(
        &mut self,
        functions: Vec<FunctionRef>,
        statics: Vec<StaticId>,
        globals_size: ProgramWord,
        function_count: ProgramWord,
    ) -> MachineTypeId {
        let key = MachineTypeKey {
            functions: functions
                .iter()
                .map(|func| (func.index, func.function_id))
                .collect(),
            statics: statics.clone(),
            globals_size,
            function_count,
        };
        let id = self.types.intern(
            key,
            MachineTypeNode {
                functions,
                statics,
                globals_size,
                function_count,
            },
        );
        MachineTypeId(id)
    }

    pub fn add_machine_instance(&mut self, type_id: MachineTypeId) {
        self.instances.push(MachineInstanceNode { type_id });
    }

    pub fn build(self) -> ProgramGraph {
        ProgramGraph {
            shared_globals_size: self.shared_globals_size,
            static_data: self.static_data.nodes,
            shared_static_data: self.shared_static_data.nodes,
            functions: self.functions.nodes,
            types: self.types.nodes,
            instances: self.instances,
            shared_functions: self.shared_functions,
            shared_function_count: self.shared_function_count,
        }
    }
}

pub struct ProgramGraph {
    shared_globals_size: ProgramWord,
    static_data: Vec<StaticDataNode>,
    shared_static_data: Vec<StaticDataNode>,
    functions: Vec<FunctionNode>,
    types: Vec<MachineTypeNode>,
    instances: Vec<MachineInstanceNode>,
    shared_functions: HashMap<ProgramWord, FunctionNode>,
    shared_function_count: ProgramWord,
}

impl ProgramGraph {
    pub fn instance_count(&self) -> ProgramWord {
        self.instances.len() as ProgramWord
    }

    pub fn type_count(&self) -> ProgramWord {
        self.types.len() as ProgramWord
    }

    pub fn shared_function_count(&self) -> ProgramWord {
        self.shared_function_count
    }

    pub fn emit_into<const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>(
        &self,
        mut builder: ProgramBuilder<'_, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    ) -> Result<ProgramDescriptor<MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>, MachineBuilderError> {
        builder.set_shared_globals_size(self.shared_globals_size)?;

        let mut shared_static_addresses: Vec<ProgramWord> =
            Vec::with_capacity(self.shared_static_data.len());
        for node in &self.shared_static_data {
            let index = builder.add_shared_static(&node.words)?;
            shared_static_addresses.push(index.to_word());
        }

        let mut static_addresses: Vec<ProgramWord> = Vec::with_capacity(self.static_data.len());
        for node in &self.static_data {
            let index = builder.add_shared_static(&node.words)?;
            static_addresses.push(index.to_word());
        }

        let mut program = builder;
        for index in 0..self.shared_function_count {
            let shared_function =
                program.new_shared_function_at_index(FunctionIndex::new(index))?;
            if let Some(function) = self.shared_functions.get(&index) {
                let shared_function = emit_shared_function(
                    shared_function,
                    function,
                    &static_addresses,
                    &shared_static_addresses,
                )?;
                let (_index, next_program) = shared_function.finish()?;
                program = next_program;
            } else {
                let mut shared_function = shared_function;
                shared_function.add_op(Op::Exit)?;
                let (_index, next_program) = shared_function.finish()?;
                program = next_program;
            }
        }

        let mut emitted_type_ids: Vec<Option<ProgramWord>> = vec![None; self.types.len()];
        let mut next_type_id: ProgramWord = 0;
        for instance in &self.instances {
            let type_id = instance.type_id.index();
            let Some(type_node) = self.types.get(type_id) else {
                continue;
            };
            if let Some(existing_id) = emitted_type_ids[type_id] {
                program.add_instance(existing_id)?;
                continue;
            }

            let mut machine = program.new_machine(type_node.function_count, type_node.globals_size)?;
            let mut functions = type_node.functions.clone();
            functions.sort_by_key(|func| func.index);
            for func in functions {
                let Some(node) = self.functions.get(func.function_id.index()) else {
                    continue;
                };
                let function_builder = machine.new_function_at_index(FunctionIndex::new(func.index))?;
                let (index, next_machine) = emit_function(
                    function_builder,
                    node,
                    &static_addresses,
                    &shared_static_addresses,
                )?;
                let _ = index;
                machine = next_machine;
            }

            let program_builder = machine.finish()?;
            emitted_type_ids[type_id] = Some(next_type_id);
            next_type_id = next_type_id
                .checked_add(1)
                .ok_or(MachineBuilderError::MachineCountOverflowsWord(
                    next_type_id as usize,
                ))?;
            program = program_builder;
        }

        Ok(program.finish_program())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use light_machine::Ops;

    #[test]
    fn dedupes_identical_types_into_one() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let function_id = builder.add_function(vec![WordRef::Literal(Ops::Exit.into())]);
        let functions = vec![FunctionRef {
            index: 0,
            function_id,
        }];
        let type_id = builder.add_machine_type(functions, Vec::new(), 0, 1);
        builder.add_machine_instance(type_id);
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        assert_eq!(graph.type_count(), 1);
        assert_eq!(graph.instance_count(), 2);

        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<4, 4>::new(&mut buffer, 2, 1, 0).unwrap();
        let descriptor = graph.emit_into(program_builder).unwrap();
        assert_eq!(descriptor.types.len(), 1);
        assert_eq!(descriptor.instances.len(), 2);
    }

    #[test]
    fn dedupes_identical_functions_across_types() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let function_id = builder.add_function(vec![WordRef::Literal(Ops::Exit.into())]);
        let type_a = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            0,
            1,
        );
        let type_b = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            1,
            1,
        );
        builder.add_machine_instance(type_a);
        builder.add_machine_instance(type_b);

        let graph = builder.build();
        assert_eq!(graph.functions.len(), 1);

        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<4, 4>::new(&mut buffer, 2, 2, 0).unwrap();
        let descriptor = graph.emit_into(program_builder).unwrap();
        assert_eq!(descriptor.types.len(), 2);
        assert_eq!(descriptor.instances.len(), 2);
    }

    #[test]
    fn dedupes_static_data_nodes() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let static_id = builder.add_static(&[1, 2, 3]);
        let function_id = builder.add_function(vec![WordRef::Static(static_id, 0), WordRef::Literal(Ops::LoadStatic.into()), WordRef::Literal(Ops::Exit.into())]);
        let type_id = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            vec![static_id],
            0,
            1,
        );
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        assert_eq!(graph.static_data.len(), 1);

        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<2, 2>::new(&mut buffer, 1, 1, 0).unwrap();
        let _descriptor = graph.emit_into(program_builder).unwrap();
    }

    #[test]
    fn dedupes_shared_static_data_nodes() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let shared_id = builder.add_shared_static(&[9, 8, 7]);
        let function_id = builder.add_function(vec![WordRef::SharedStatic(shared_id, 1), WordRef::Literal(Ops::LoadStatic.into()), WordRef::Literal(Ops::Exit.into())]);
        let type_id = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            0,
            1,
        );
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        assert_eq!(graph.shared_static_data.len(), 1);

        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<2, 2>::new(&mut buffer, 1, 1, 0).unwrap();
        let _descriptor = graph.emit_into(program_builder).unwrap();
    }

    #[test]
    fn emits_shared_function_table_entries() {
        let mut builder = ProgramGraphBuilder::new(2);
        builder.set_shared_globals_size(0);
        builder
            .add_shared_function(1, vec![WordRef::Literal(Ops::Exit.into())])
            .unwrap();
        let type_id = builder.add_machine_type(Vec::new(), Vec::new(), 0, 0);
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        assert_eq!(graph.shared_function_count(), 2);

        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<2, 2>::new(&mut buffer, 1, 1, 2).unwrap();
        let descriptor = graph.emit_into(program_builder).unwrap();
        assert_eq!(descriptor.instances.len(), 1);
    }

    #[test]
    fn instance_order_is_preserved_across_deduped_types() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let function_id = builder.add_function(vec![WordRef::Literal(Ops::Exit.into())]);
        let type_id = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            0,
            1,
        );
        builder.add_machine_instance(type_id);
        builder.add_machine_instance(type_id);
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<4, 4>::new(&mut buffer, 3, 1, 0).unwrap();
        let descriptor = graph.emit_into(program_builder).unwrap();
        assert_eq!(descriptor.instances.len(), 3);
        assert_eq!(descriptor.types.len(), 1);
    }

    #[test]
    fn different_globals_size_prevents_type_dedupe() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let function_id = builder.add_function(vec![WordRef::Literal(Ops::Exit.into())]);
        let type_a = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            0,
            1,
        );
        let type_b = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            2,
            1,
        );
        builder.add_machine_instance(type_a);
        builder.add_machine_instance(type_b);

        let graph = builder.build();
        assert_eq!(graph.type_count(), 2);

        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<4, 4>::new(&mut buffer, 2, 2, 0).unwrap();
        let descriptor = graph.emit_into(program_builder).unwrap();
        assert_eq!(descriptor.types.len(), 2);
    }

    #[test]
    fn shared_static_offsets_resolve() {
        let mut builder = ProgramGraphBuilder::new(0);
        builder.set_shared_globals_size(0);
        let shared_id = builder.add_shared_static(&[10, 20, 30]);
        let words = vec![
            WordRef::SharedStatic(shared_id, 2),
            WordRef::Literal(Ops::LoadStatic.into()),
            WordRef::Literal(Ops::Exit.into()),
        ];
        let function_id = builder.add_function(words);
        let type_id = builder.add_machine_type(
            vec![FunctionRef { index: 0, function_id }],
            Vec::new(),
            0,
            1,
        );
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<2, 2>::new(&mut buffer, 1, 1, 0).unwrap();
        let _descriptor = graph.emit_into(program_builder).unwrap();
    }

    #[test]
    fn shared_function_gaps_do_not_break_emit() {
        let mut builder = ProgramGraphBuilder::new(3);
        builder.set_shared_globals_size(0);
        builder
            .add_shared_function(2, vec![WordRef::Literal(Ops::Exit.into())])
            .unwrap();
        let type_id = builder.add_machine_type(Vec::new(), Vec::new(), 0, 0);
        builder.add_machine_instance(type_id);

        let graph = builder.build();
        let mut buffer = [0u16; 128];
        let program_builder = ProgramBuilder::<2, 2>::new(&mut buffer, 1, 1, 3).unwrap();
        let descriptor = graph.emit_into(program_builder).unwrap();
        assert_eq!(descriptor.instances.len(), 1);
    }
}
fn resolve_word(
    word: &WordRef,
    function_start: ProgramWord,
    static_addresses: &[ProgramWord],
    shared_static_addresses: &[ProgramWord],
) -> Result<ProgramWord, MachineBuilderError> {
    match *word {
        WordRef::Literal(value) => Ok(value),
        WordRef::LabelOffset(offset) => function_start
            .checked_add(offset)
            .ok_or(MachineBuilderError::TooLarge(offset as usize)),
        WordRef::Static(id, offset) => static_addresses
            .get(id.index())
            .copied()
            .ok_or(MachineBuilderError::BufferTooSmall)
            .and_then(|base| {
                base.checked_add(offset)
                    .ok_or(MachineBuilderError::TooLarge(offset as usize))
            }),
        WordRef::SharedStatic(id, offset) => shared_static_addresses
            .get(id.index())
            .copied()
            .ok_or(MachineBuilderError::BufferTooSmall)
            .and_then(|base| {
                base.checked_add(offset)
                    .ok_or(MachineBuilderError::TooLarge(offset as usize))
            }),
    }
}

fn emit_function<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>(
    mut function: light_machine::builder::FunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    node: &FunctionNode,
    static_addresses: &[ProgramWord],
    shared_static_addresses: &[ProgramWord],
) -> Result<
    (
        light_machine::builder::FunctionIndex,
        light_machine::builder::MachineBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    ),
    MachineBuilderError,
> {
    let function_start = function.function_start();
    for word in &node.words {
        let resolved = resolve_word(word, function_start, static_addresses, shared_static_addresses)?;
        function.add_raw_word(resolved)?;
    }
    function.finish()
}

fn emit_shared_function<'a, const MACHINE_COUNT_MAX: usize, const FUNCTION_COUNT_MAX: usize>(
    mut function: light_machine::builder::SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    node: &FunctionNode,
    static_addresses: &[ProgramWord],
    shared_static_addresses: &[ProgramWord],
) -> Result<
    light_machine::builder::SharedFunctionBuilder<'a, MACHINE_COUNT_MAX, FUNCTION_COUNT_MAX>,
    MachineBuilderError,
> {
    let function_start = function.function_start();
    for word in &node.words {
        let resolved = resolve_word(word, function_start, static_addresses, shared_static_addresses)?;
        function.add_raw_word(resolved)?;
    }
    Ok(function)
}
