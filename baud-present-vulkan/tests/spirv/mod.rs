//! **A SPIR-V module's uniform blocks, read from its instructions**: a
//! block's members and their offsets, and the set and binding of its
//! variable, for the tests that hold a shader's uniform to the Rust that
//! writes it.

#![allow(dead_code)]

use std::collections::HashMap;

/// **What a module says about a uniform block**: the members' names and
/// offsets in order, and the set and binding of the variable of that type.
#[derive(Debug, PartialEq)]
pub struct Block {
    pub members: Vec<(String, u32)>,
    pub set: DescriptorSet,
    pub binding: Binding,
}

/// A `DescriptorSet` decoration's operand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DescriptorSet(pub u32);

/// A `Binding` decoration's operand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binding(pub u32);

/// A SPIR-V string operand: UTF-8, nul-terminated, in little-endian words.
fn string(words: &[u32]) -> String {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// **The block named `name` in a SPIR-V module**, read from its instructions
/// (`OpName` 5, `OpMemberName` 6, `OpDecorate` 71, `OpMemberDecorate` 72,
/// `OpTypePointer` 32, `OpVariable` 59; decorations `Binding` 33,
/// `DescriptorSet` 34, `Offset` 35), after the five-word header.
pub fn block(spv: &[u8], name: &str) -> Option<Block> {
    let words: Vec<u32> = spv
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    assert_eq!(words.first(), Some(&0x0723_0203), "SPIR-V's magic number");
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut member_names: HashMap<(u32, u32), String> = HashMap::new();
    let mut offsets: HashMap<(u32, u32), u32> = HashMap::new();
    let mut decorations: HashMap<(u32, u32), u32> = HashMap::new();
    let mut pointers: HashMap<u32, u32> = HashMap::new();
    let mut variables: Vec<(u32, u32)> = Vec::new();
    let mut at = 5;
    while at < words.len() {
        let count = usize::try_from(words[at] >> 16).ok()?;
        let op = words[at] & 0xffff;
        let ins = words.get(at..at + count)?;
        match op {
            5 => {
                names.insert(ins[1], string(&ins[2..]));
            }
            6 => {
                member_names.insert((ins[1], ins[2]), string(&ins[3..]));
            }
            71 if ins.len() >= 4 => {
                decorations.insert((ins[1], ins[2]), ins[3]);
            }
            72 if ins.len() >= 5 && ins[3] == 35 => {
                offsets.insert((ins[1], ins[2]), ins[4]);
            }
            32 => {
                pointers.insert(ins[1], ins[3]);
            }
            59 => variables.push((ins[1], ins[2])),
            _ => {}
        }
        at += count.max(1);
    }
    let block = *names.iter().find(|(_, n)| n.as_str() == name)?.0;
    let mut members: Vec<(u32, String, u32)> = member_names
        .iter()
        .filter(|((s, _), _)| *s == block)
        .map(|((_, m), n)| {
            (
                *m,
                n.clone(),
                offsets.get(&(block, *m)).copied().unwrap_or(u32::MAX),
            )
        })
        .collect();
    members.sort();
    let (_, var) = variables
        .iter()
        .find(|(ty, _)| pointers.get(ty) == Some(&block))?;
    Some(Block {
        members: members.into_iter().map(|(_, n, o)| (n, o)).collect(),
        set: DescriptorSet(*decorations.get(&(*var, 34))?),
        binding: Binding(*decorations.get(&(*var, 33))?),
    })
}

/// A resource variable's storage class, as SPIR-V numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// `UniformConstant` (0): a sampled image.
    Sampler,
    /// `Uniform` (2): a uniform block.
    Uniform,
    /// `StorageBuffer` (12).
    Storage,
}

/// **What a module declares**: its stage (`OpEntryPoint`'s execution
/// model, 0 vertex, 4 fragment), every resource variable with its set,
/// binding and class, and whether it declares a push-constant block.
#[derive(Debug)]
pub struct Declared {
    pub model: u32,
    pub resources: Vec<(DescriptorSet, Binding, Class)>,
    pub push: bool,
}

/// What `spv` declares, read from `OpEntryPoint` (15), `OpVariable` (59)
/// and the `Binding` and `DescriptorSet` decorations.
pub fn declared(spv: &[u8]) -> Declared {
    let words: Vec<u32> = spv
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    assert_eq!(words.first(), Some(&0x0723_0203), "SPIR-V's magic number");
    let mut model = None;
    let mut decorations: HashMap<(u32, u32), u32> = HashMap::new();
    let mut variables: Vec<(u32, u32)> = Vec::new();
    let mut at = 5;
    while at < words.len() {
        let count = usize::try_from(words[at] >> 16).expect("a word count");
        let ins = &words[at..at + count];
        match words[at] & 0xffff {
            15 => model = Some(ins[1]),
            71 if ins.len() >= 4 => {
                decorations.insert((ins[1], ins[2]), ins[3]);
            }
            59 => variables.push((ins[2], ins[3])),
            _ => {}
        }
        at += count.max(1);
    }
    let mut resources = Vec::new();
    let mut push = false;
    for (id, class) in variables {
        let class = match class {
            0 => Class::Sampler,
            2 => Class::Uniform,
            12 => Class::Storage,
            9 => {
                push = true;
                continue;
            }
            _ => continue,
        };
        let set = decorations.get(&(id, 34)).copied().expect("a set");
        let binding = decorations.get(&(id, 33)).copied().expect("a binding");
        resources.push((DescriptorSet(set), Binding(binding), class));
    }
    resources.sort_by_key(|(s, b, _)| (s.0, b.0));
    Declared {
        model: model.expect("an entry point"),
        resources,
        push,
    }
}

/// A size in bytes, as a SPIR-V module lays a block out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bytes(pub u32);

/// **The bytes a module's push-constant block spans**: the end of its
/// last member, each member a scalar or a vector, read from `OpTypeInt`
/// (21), `OpTypeFloat` (22), `OpTypeVector` (23), `OpTypeStruct` (30),
/// `OpTypePointer` (32), `OpVariable` (59) of storage class
/// `PushConstant` (9) and the members' `Offset` decorations; none where
/// the module declares no push constant.
pub fn push_bytes(spv: &[u8]) -> Option<Bytes> {
    let words: Vec<u32> = spv
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    assert_eq!(words.first(), Some(&0x0723_0203), "SPIR-V's magic number");
    let mut sizes: HashMap<u32, u32> = HashMap::new();
    let mut structs: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut offsets: HashMap<(u32, u32), u32> = HashMap::new();
    let mut pointers: HashMap<u32, u32> = HashMap::new();
    let mut pushed = None;
    let mut at = 5;
    while at < words.len() {
        let count = usize::try_from(words[at] >> 16).expect("a word count");
        let ins = &words[at..at + count];
        match words[at] & 0xffff {
            21 | 22 => {
                sizes.insert(ins[1], ins[2] / 8);
            }
            23 => {
                let component = *sizes.get(&ins[2]).expect("a vector's component");
                sizes.insert(ins[1], component * ins[3]);
            }
            30 => {
                structs.insert(ins[1], ins[2..].to_vec());
            }
            32 => {
                pointers.insert(ins[1], ins[3]);
            }
            59 if ins[3] == 9 => pushed = Some(ins[1]),
            72 if ins.len() >= 5 && ins[3] == 35 => {
                offsets.insert((ins[1], ins[2]), ins[4]);
            }
            _ => {}
        }
        at += count.max(1);
    }
    let block = *pointers.get(&pushed?).expect("the push constant's type");
    let members = structs.get(&block).expect("a push-constant block");
    let end = (0u32..)
        .zip(members)
        .map(|(m, ty)| {
            let offset = offsets.get(&(block, m)).expect("a member's offset");
            offset + sizes.get(ty).expect("a scalar or vector member")
        })
        .max()
        .unwrap_or(0);
    Some(Bytes(end))
}
