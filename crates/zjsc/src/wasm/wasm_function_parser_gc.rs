//! Parte de `wasm/WasmFunctionParser.h`: as instruções de GC (`0xFB`) e atômicas (`0xFE`), nos dois
//! modos (código alcançável e inalcançável). Ver `wasm_function_parser.rs` para as diferenças em
//! relação ao C++.

use crate::wasm::wasm_format::{
    FieldType, Mutability, PackedType, StorageType, Type, TypeIndex, TypeKind, is_defaultable_type, is_ref_type,
};
use crate::wasm::wasm_function_parser::{
    Context, FunctionParser, Hook, PartialResult, parse_or_fail, pfail_if, pop_or_fail, simple_type, unpacked, vfail_if,
};
use crate::wasm::wasm_limits::MAX_ARRAY_NEW_FIXED_ARGS;
use crate::wasm::wasm_module_information::{RttKind, StructuralType};
use crate::wasm::wasm_ops::{ExtAtomicOpType, ExtGCOpType};

fn ref_type(nullable: bool, kind: TypeKind) -> Type {
    Type::new(if nullable { TypeKind::RefNull } else { TypeKind::Ref }, TypeIndex::Abstract(kind))
}

fn is_ref_to(ty: Type, kind: TypeKind) -> bool {
    is_ref_type(ty) && ty.index == TypeIndex::Abstract(kind)
}

fn packed_of(storage: StorageType) -> Option<PackedType> {
    match storage {
        StorageType::Packed(packed) => Some(packed),
        StorageType::Type(_) => None,
    }
}

impl<'s, 'i, C: Context> FunctionParser<'s, 'i, C> {
    fn skip_u32(&mut self, message: &str) -> PartialResult {
        parse_or_fail!(self, self.parser.parse_var_uint32(), "{}", message);
        Ok(())
    }

    /// `parseStructTypeIndex`: devolve a posição e os campos.
    fn parse_struct_type_index(&mut self, operation: &str) -> Result<(u32, Vec<FieldType>), String> {
        let type_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get type index for {}", operation);
        vfail_if!(self, type_index as usize >= self.info.type_count(), "{} index {} is out of bound", operation, type_index);
        match &self.info.rtt(type_index as usize).structural {
            StructuralType::Struct { fields } => Ok((type_index, fields.clone())),
            _ => self.vfail(format!("{}: invalid type index {}", operation, type_index)),
        }
    }

    fn parse_struct_type_index_and_field_index(&mut self, operation: &str) -> Result<(u32, u32, Vec<FieldType>), String> {
        let (type_index, fields) = self.parse_struct_type_index(operation)?;
        let field_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get type index for {}", operation);
        pfail_if!(
            self,
            field_index as usize >= fields.len(),
            "{} field immediate {} is out of bounds",
            operation,
            field_index
        );
        Ok((type_index, field_index, fields))
    }

    /// `parseStructFieldManipulation`: (posição do tipo, campo, referência, tipo do campo).
    fn parse_struct_field_manipulation(&mut self, operation: &str) -> Result<(u32, u32, Type, FieldType), String> {
        let (type_index, field_index, fields) = self.parse_struct_type_index_and_field_index(operation)?;
        let struct_ref = pop_or_fail!(self, "struct reference");
        let struct_ref_type = Type::new(TypeKind::RefNull, self.info.type_index_of(type_index as usize));
        vfail_if!(
            self,
            !self.info.is_subtype(struct_ref, struct_ref_type),
            "{} structref to type {} expected {}",
            operation,
            self.ty(struct_ref),
            self.ty(struct_ref_type)
        );
        Ok((type_index, field_index, struct_ref, fields[field_index as usize]))
    }

    /// `parseArrayTypeDefinition`: (posição, tipo do elemento, tipo de referência ao array).
    fn parse_array_type_definition(&mut self, operation: &str, is_nullable: bool) -> Result<(u32, FieldType, Type), String> {
        let raw_type_index = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get type index for {}", operation);
        vfail_if!(self, raw_type_index as usize >= self.info.type_count(), "{} index {} is out of bounds", operation, raw_type_index);
        let element = match &self.info.rtt(raw_type_index as usize).structural {
            StructuralType::Array { element } => *element,
            _ => {
                return self.vfail(format!("{} index {} does not reference an array definition", operation, raw_type_index));
            }
        };
        let array_ref_type = Type::new(
            if is_nullable { TypeKind::RefNull } else { TypeKind::Ref },
            self.info.type_index_of(raw_type_index as usize),
        );
        Ok((raw_type_index, element, array_ref_type))
    }

    fn expect_i32(&mut self, value: Type, what: &str) -> PartialResult {
        vfail_if!(
            self,
            TypeKind::I32 != value.kind,
            "{} to type {} expected {}",
            what,
            self.ty(value),
            TypeKind::I32.name()
        );
        Ok(())
    }

    /// Os argumentos de `struct.new`/`array.new_fixed` já conferidos, desempilhados.
    fn pop_typed_arguments(&mut self, expected: &[Type], message: &str) -> PartialResult {
        let count = expected.len();
        for i in 0..count {
            let arg = self.expression_stack[self.expression_stack.len() - i - 1];
            let wanted = expected[count - i - 1];
            vfail_if!(
                self,
                !self.info.is_subtype(arg, wanted),
                "{}, got {}, expected {}",
                message,
                self.ty(arg),
                self.ty(wanted)
            );
        }
        let new_len = self.expression_stack.len() - count;
        self.expression_stack.truncate(new_len);
        Ok(())
    }

    fn heap_type_index_of(&self, heap_type: i32) -> TypeIndex {
        self.heap_type_index(heap_type)
    }

    /// O `case ExtGC` de `parseExpression`.
    pub(super) fn parse_ext_gc(&mut self) -> PartialResult {
        self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse extended GC opcode");
        let ext = self.current_ext_op;
        let Some(op) = ExtGCOpType::from_value(ext) else {
            return self.pfail(format!("invalid extended GC op {}", ext));
        };
        let i32_type = simple_type(TypeKind::I32);
        match op {
            ExtGCOpType::RefI31 => {
                let value = pop_or_fail!(self, "ref.i31");
                vfail_if!(
                    self,
                    !value.is_i32(),
                    "ref.i31 value to type {} expected {}",
                    self.ty(value),
                    TypeKind::I32.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(Type::new(TypeKind::Ref, TypeIndex::Abstract(TypeKind::I31ref)));
            }
            ExtGCOpType::I31GetS | ExtGCOpType::I31GetU => {
                let name = if op == ExtGCOpType::I31GetS { "i31.get_s" } else { "i31.get_u" };
                let reference = pop_or_fail!(self, name);
                vfail_if!(
                    self,
                    !is_ref_to(reference, TypeKind::I31ref),
                    "{} ref to type {} expected {}",
                    name,
                    self.ty(reference),
                    TypeKind::I31ref.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(i32_type);
            }
            ExtGCOpType::ArrayNew => {
                let (_, field, array_ref_type) = self.parse_array_type_definition("array.new", false)?;
                let element = unpacked(field.ty);
                let size = pop_or_fail!(self, "array.new");
                let value = pop_or_fail!(self, "array.new");
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, element),
                    "array.new value to type {} expected {}",
                    self.ty(value),
                    self.ty(element)
                );
                vfail_if!(
                    self,
                    TypeKind::I32 != size.kind,
                    "array.new index to type {} expected {}",
                    self.ty(size),
                    TypeKind::I32.name()
                );
                if element.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(array_ref_type);
            }
            ExtGCOpType::ArrayNewDefault => {
                let (type_index, field, array_ref_type) = self.parse_array_type_definition("array.new_default", false)?;
                let defaultable = match field.ty {
                    StorageType::Type(ty) => is_defaultable_type(ty),
                    StorageType::Packed(_) => true,
                };
                vfail_if!(
                    self,
                    !defaultable,
                    "array.new_default index {} does not reference an array definition with a defaultable type",
                    type_index
                );
                let size = pop_or_fail!(self, "array.new_default");
                vfail_if!(
                    self,
                    TypeKind::I32 != size.kind,
                    "array.new_default index to type {} expected {}",
                    self.ty(size),
                    TypeKind::I32.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(array_ref_type);
            }
            ExtGCOpType::ArrayNewFixed => {
                let (_, field, array_ref_type) = self.parse_array_type_definition("array.new_fixed", false)?;
                let element = unpacked(field.ty);
                let argc = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't get argument count for array.new_fixed");
                vfail_if!(
                    self,
                    argc as usize > MAX_ARRAY_NEW_FIXED_ARGS,
                    "array_new_fixed can take at most {} operands. Got {}",
                    MAX_ARRAY_NEW_FIXED_ARGS,
                    argc
                );
                let slice_size = self.slice_size();
                vfail_if!(
                    self,
                    argc as usize > slice_size,
                    "array_new_fixed: found {} operands on stack; expected {} operands",
                    slice_size,
                    argc
                );
                for i in 0..argc as usize {
                    let arg = self.expression_stack[self.expression_stack.len() - i - 1];
                    vfail_if!(
                        self,
                        !self.info.is_subtype(arg, element),
                        "argument type mismatch in array.new_fixed, got {}, expected a subtype of {}",
                        self.ty(arg),
                        self.ty(element)
                    );
                }
                let new_len = self.expression_stack.len() - argc as usize;
                self.expression_stack.truncate(new_len);
                if element.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(array_ref_type);
            }
            ExtGCOpType::ArrayNewData => {
                let (_, field, array_ref_type) = self.parse_array_type_definition("array.new_data", false)?;
                if let StorageType::Type(ty) = field.ty {
                    vfail_if!(
                        self,
                        is_ref_type(ty),
                        "array.new_data expected numeric, packed, or vector type; found {}",
                        self.ty(ty)
                    );
                }
                let data_index = parse_or_fail!(
                    self,
                    self.parser.parse_var_uint32(),
                    "can't get data segment index for array.new_data"
                );
                vfail_if!(self, self.info.data_segments_count() == 0, "array.new_data in module with no data segments");
                vfail_if!(
                    self,
                    data_index >= self.info.data_segments_count(),
                    "array.new_data segment index {} is out of bounds (maximum data segment index is {})",
                    data_index,
                    self.info.data_segments_count() - 1
                );
                let size = pop_or_fail!(self, "array.new_data");
                vfail_if!(
                    self,
                    TypeKind::I32 != size.kind,
                    "array.new_data: size has type {} expected {}",
                    size.kind.name(),
                    TypeKind::I32.name()
                );
                let offset = pop_or_fail!(self, "array.new_data");
                vfail_if!(
                    self,
                    TypeKind::I32 != offset.kind,
                    "array.new_data: offset has type {} expected {}",
                    offset.kind.name(),
                    TypeKind::I32.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(array_ref_type);
            }
            ExtGCOpType::ArrayNewElem => {
                let (_, field, array_ref_type) = self.parse_array_type_definition("array.new_elem", false)?;
                let segment_index = parse_or_fail!(
                    self,
                    self.parser.parse_var_uint32(),
                    "can't get elements segment index for array.new_elem"
                );
                let number_of_segments = self.info.elements.len();
                vfail_if!(self, number_of_segments == 0, "array.new_elem in module with no elements segments");
                vfail_if!(
                    self,
                    segment_index as usize >= number_of_segments,
                    "array.new_elem segment index {} is out of bounds (maximum element segment index is {})",
                    segment_index,
                    number_of_segments - 1
                );
                let segment_type = self.info.elements[segment_index as usize].element_type;
                vfail_if!(
                    self,
                    packed_of(field.ty).is_some(),
                    "type mismatch in array.new_elem: expected `funcref` or `externref`"
                );
                let element = unpacked(field.ty);
                vfail_if!(
                    self,
                    !self.info.is_subtype(segment_type, element),
                    "type mismatch in array.new_elem: segment elements have type {} but array.new_elem operation expects elements of type {}",
                    self.ty(segment_type),
                    self.ty(element)
                );
                let size = pop_or_fail!(self, "array.new_elem");
                vfail_if!(
                    self,
                    TypeKind::I32 != size.kind,
                    "array.new_elem: size has type {} expected {}",
                    size.kind.name(),
                    TypeKind::I32.name()
                );
                let offset = pop_or_fail!(self, "array.new_elem");
                vfail_if!(
                    self,
                    TypeKind::I32 != offset.kind,
                    "array.new_elem: offset has type {} expected {}",
                    offset.kind.name(),
                    TypeKind::I32.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(array_ref_type);
            }
            ExtGCOpType::ArrayGet | ExtGCOpType::ArrayGetS | ExtGCOpType::ArrayGetU => {
                let op_name = match op {
                    ExtGCOpType::ArrayGet => "array.get",
                    ExtGCOpType::ArrayGetS => "array.get_s",
                    _ => "array.get_u",
                };
                let (_, field, array_ref_type) = self.parse_array_type_definition(op_name, true)?;
                // array.get_s and array.get_u are only valid for packed arrays
                if op != ExtGCOpType::ArrayGet {
                    pfail_if!(
                        self,
                        packed_of(field.ty).is_none(),
                        "{} applied to wrong type of array -- expected: i8 or i16, found {}",
                        op_name,
                        match field.ty {
                            StorageType::Type(ty) => ty.kind.name(),
                            StorageType::Packed(_) => "",
                        }
                    );
                }
                // array.get is not valid for packed arrays
                if op == ExtGCOpType::ArrayGet {
                    if let Some(packed) = packed_of(field.ty) {
                        return self.pfail(format!(
                            "{} applied to packed array of {} -- use array.get_s or array.get_u",
                            op_name,
                            packed.name()
                        ));
                    }
                }
                let result_type = unpacked(field.ty);
                let index = pop_or_fail!(self, "array.get");
                let arrayref = pop_or_fail!(self, "array.get");
                vfail_if!(
                    self,
                    !self.info.is_subtype(arrayref, array_ref_type),
                    "{} arrayref to type {} expected {}",
                    op_name,
                    self.ty(arrayref),
                    self.ty(array_ref_type)
                );
                vfail_if!(
                    self,
                    TypeKind::I32 != index.kind,
                    "array.get index to type {} expected {}",
                    self.ty(index),
                    TypeKind::I32.name()
                );
                if result_type.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(result_type);
            }
            ExtGCOpType::ArraySet => {
                let (type_index, field, array_ref_type) = self.parse_array_type_definition("array.set", true)?;
                let element = unpacked(field.ty);
                vfail_if!(
                    self,
                    field.mutability != Mutability::Mutable,
                    "array.set index {} does not reference a mutable array definition",
                    type_index
                );
                let value = pop_or_fail!(self, "array.set");
                let index = pop_or_fail!(self, "array.set");
                let arrayref = pop_or_fail!(self, "array.set");
                vfail_if!(
                    self,
                    !self.info.is_subtype(arrayref, array_ref_type),
                    "array.set arrayref to type {} expected {}",
                    self.ty(arrayref),
                    self.ty(array_ref_type)
                );
                vfail_if!(
                    self,
                    TypeKind::I32 != index.kind,
                    "array.set index to type {} expected {}",
                    self.ty(index),
                    TypeKind::I32.name()
                );
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, element),
                    "array.set value to type {} expected {}",
                    self.ty(value),
                    self.ty(element)
                );
                if element.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::ArrayLen => {
                let arrayref = pop_or_fail!(self, "array.len");
                vfail_if!(
                    self,
                    !self.info.is_subtype(arrayref, ref_type(true, TypeKind::Arrayref)),
                    "array.len value to type {} expected arrayref",
                    self.ty(arrayref)
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(i32_type);
            }
            ExtGCOpType::ArrayFill => {
                let (type_index, field, array_ref_type) = self.parse_array_type_definition("array.fill", true)?;
                let element = unpacked(field.ty);
                vfail_if!(
                    self,
                    field.mutability != Mutability::Mutable,
                    "array.fill index {} does not reference a mutable array definition",
                    type_index
                );
                let size = pop_or_fail!(self, "array.fill");
                let value = pop_or_fail!(self, "array.fill");
                let offset = pop_or_fail!(self, "array.fill");
                let arrayref = pop_or_fail!(self, "array.fill");
                vfail_if!(
                    self,
                    !self.info.is_subtype(arrayref, array_ref_type),
                    "array.fill arrayref to type {} expected {}",
                    self.ty(arrayref),
                    self.ty(array_ref_type)
                );
                self.expect_i32(offset, "array.fill offset")?;
                vfail_if!(
                    self,
                    !self.info.is_subtype(value, element),
                    "array.fill value to type {} expected {}",
                    self.ty(value),
                    self.ty(element)
                );
                self.expect_i32(size, "array.fill size")?;
                if element.is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::ArrayCopy => {
                let (dst_type_index, dst_field, dst_ref_type) = self.parse_array_type_definition("array.copy", true)?;
                let (src_type_index, src_field, src_ref_type) = self.parse_array_type_definition("array.copy", true)?;
                vfail_if!(
                    self,
                    dst_field.mutability != Mutability::Mutable,
                    "array.copy index {} does not reference a mutable array definition",
                    dst_type_index
                );
                vfail_if!(
                    self,
                    !self.info.is_subtype_storage(src_field.ty, dst_field.ty),
                    "array.copy src index {} does not reference a subtype of dst index {}",
                    src_type_index,
                    dst_type_index
                );
                let size = pop_or_fail!(self, "array.copy");
                let src_offset = pop_or_fail!(self, "array.copy");
                let src = pop_or_fail!(self, "array.copy");
                let dst_offset = pop_or_fail!(self, "array.copy");
                let dst = pop_or_fail!(self, "array.copy");
                vfail_if!(
                    self,
                    !self.info.is_subtype(dst, dst_ref_type),
                    "array.copy dst to type {} expected {}",
                    self.ty(dst),
                    self.ty(dst_ref_type)
                );
                self.expect_i32(dst_offset, "array.copy dstOffset")?;
                vfail_if!(
                    self,
                    !self.info.is_subtype(src, src_ref_type),
                    "array.copy src to type {} expected {}",
                    self.ty(src),
                    self.ty(src_ref_type)
                );
                self.expect_i32(src_offset, "array.copy srcOffset")?;
                self.expect_i32(size, "array.copy size")?;
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::ArrayInitElem => {
                let (dst_type_index, dst_field, dst_ref_type) = self.parse_array_type_definition("array.init_elem", true)?;
                let segment_index = self.parse_element_index()?;
                let segment_type = self.info.elements[segment_index as usize].element_type;
                let element = unpacked(dst_field.ty);
                vfail_if!(
                    self,
                    dst_field.mutability != Mutability::Mutable,
                    "array.init_elem index {} does not reference a mutable array definition",
                    dst_type_index
                );
                vfail_if!(
                    self,
                    !self.info.is_subtype(segment_type, element),
                    "type mismatch in array.init_elem: segment elements have type {} but array.init_elem operation expects elements of type {}",
                    self.ty(segment_type),
                    self.ty(element)
                );
                let size = pop_or_fail!(self, "array.init_elem");
                let src_offset = pop_or_fail!(self, "array.init_elem");
                let dst_offset = pop_or_fail!(self, "array.init_elem");
                let dst = pop_or_fail!(self, "array.init_elem");
                vfail_if!(
                    self,
                    !self.info.is_subtype(dst, dst_ref_type),
                    "array.init_elem dst to type {} expected {}",
                    self.ty(dst),
                    self.ty(dst_ref_type)
                );
                self.expect_i32(dst_offset, "array.init_elem dstOffset")?;
                self.expect_i32(src_offset, "array.init_elem srcOffset")?;
                self.expect_i32(size, "array.init_elem size")?;
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::ArrayInitData => {
                let (dst_type_index, dst_field, dst_ref_type) = self.parse_array_type_definition("array.init_data", true)?;
                self.parse_data_segment_index()?;
                vfail_if!(
                    self,
                    dst_field.mutability != Mutability::Mutable,
                    "array.init_data index {} does not reference a mutable array definition",
                    dst_type_index
                );
                vfail_if!(
                    self,
                    packed_of(dst_field.ty).is_none() && is_ref_type(unpacked(dst_field.ty)),
                    "array.init_data index {} must refer to an array definition with numeric or vector type",
                    dst_type_index
                );
                let size = pop_or_fail!(self, "array.init_data");
                let src_offset = pop_or_fail!(self, "array.init_data");
                let dst_offset = pop_or_fail!(self, "array.init_data");
                let dst = pop_or_fail!(self, "array.init_data");
                vfail_if!(
                    self,
                    !self.info.is_subtype(dst, dst_ref_type),
                    "array.init_data dst to type {} expected {}",
                    self.ty(dst),
                    self.ty(dst_ref_type)
                );
                self.expect_i32(dst_offset, "array.init_data dstOffset")?;
                self.expect_i32(src_offset, "array.init_data srcOffset")?;
                self.expect_i32(size, "array.init_data size")?;
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::StructNew => {
                let (type_index, fields) = self.parse_struct_type_index("struct.new")?;
                let slice_size = self.slice_size();
                pfail_if!(
                    self,
                    fields.len() > slice_size,
                    "struct.new {} requires {} values, but the expression stack currently holds {} values",
                    type_index,
                    fields.len(),
                    slice_size
                );
                let expected: Vec<Type> = fields.iter().map(|field| unpacked(field.ty)).collect();
                self.pop_typed_arguments(&expected, "argument type mismatch in struct.new")?;
                if expected.iter().any(|ty| ty.is_v128()) {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(Type::new(TypeKind::Ref, self.info.type_index_of(type_index as usize)));
            }
            ExtGCOpType::StructNewDefault => {
                let (type_index, fields) = self.parse_struct_type_index("struct.new_default")?;
                for (i, field) in fields.iter().enumerate() {
                    let defaultable = match field.ty {
                        StorageType::Type(ty) => is_defaultable_type(ty),
                        StorageType::Packed(_) => true,
                    };
                    pfail_if!(
                        self,
                        !defaultable,
                        "struct.new_default {} requires all fields to be defaultable, but field {} has type {}",
                        type_index,
                        i,
                        match field.ty {
                            StorageType::Type(ty) => self.ty(ty),
                            StorageType::Packed(packed) => packed.name().to_string(),
                        }
                    );
                }
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(Type::new(TypeKind::Ref, self.info.type_index_of(type_index as usize)));
            }
            ExtGCOpType::StructGet | ExtGCOpType::StructGetS | ExtGCOpType::StructGetU => {
                let op_name = match op {
                    ExtGCOpType::StructGet => "struct.get",
                    ExtGCOpType::StructGetS => "struct.get_s",
                    _ => "struct.get_u",
                };
                let (_, _, _, field) = self.parse_struct_field_manipulation(op_name)?;
                if op != ExtGCOpType::StructGet {
                    pfail_if!(
                        self,
                        packed_of(field.ty).is_none(),
                        "{} applied to wrong type of struct -- expected: i8 or i16, found {}",
                        op_name,
                        match field.ty {
                            StorageType::Type(ty) => ty.kind.name(),
                            StorageType::Packed(_) => "",
                        }
                    );
                }
                if op == ExtGCOpType::StructGet {
                    if let Some(packed) = packed_of(field.ty) {
                        return self.pfail(format!(
                            "{} applied to packed array of {} -- use struct.get_s or struct.get_u",
                            op_name,
                            packed.name()
                        ));
                    }
                }
                if unpacked(field.ty).is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(unpacked(field.ty));
            }
            ExtGCOpType::StructSet => {
                let value = pop_or_fail!(self, "struct.set value");
                let (_, field_index, _, field) = self.parse_struct_field_manipulation("struct.set")?;
                pfail_if!(
                    self,
                    field.mutability != Mutability::Mutable,
                    "the field {} can't be set because it is immutable",
                    field_index
                );
                pfail_if!(self, !self.info.is_subtype(value, unpacked(field.ty)), "type mismatch in struct.set");
                if unpacked(field.ty).is_v128() {
                    self.context.notify_function_uses_simd();
                }
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::RefTest | ExtGCOpType::RefTestNull | ExtGCOpType::RefCast | ExtGCOpType::RefCastNull => {
                let op_name = if op == ExtGCOpType::RefCast || op == ExtGCOpType::RefCastNull { "ref.cast" } else { "ref.test" };
                let heap_type = parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "can't get heap type for {}",
                    op_name
                );
                let reference = pop_or_fail!(self, op_name);
                vfail_if!(
                    self,
                    !is_ref_type(reference),
                    "{} to type {} expected a reference type",
                    op_name,
                    self.ty(reference)
                );
                let result_type_index;
                if heap_type < 0 {
                    let kind = TypeKind::from_i8(heap_type as i8).expect("tipo heap já validado");
                    result_type_index = TypeIndex::Abstract(kind);
                    let (needed, expected) = match kind {
                        TypeKind::Funcref | TypeKind::Nofuncref => (ref_type(true, TypeKind::Funcref), "a funcref"),
                        TypeKind::Externref | TypeKind::Noexternref => (ref_type(true, TypeKind::Externref), "an externref"),
                        TypeKind::Exnref | TypeKind::Noexnref => (ref_type(true, TypeKind::Exnref), "an exnref"),
                        _ => (ref_type(true, TypeKind::Anyref), "a subtype of anyref"),
                    };
                    let name = if needed.index == TypeIndex::Abstract(TypeKind::Anyref) { "ref.cast" } else { op_name };
                    vfail_if!(
                        self,
                        !self.info.is_subtype(reference, needed),
                        "{} to type {} expected {}",
                        name,
                        self.ty(reference),
                        expected
                    );
                } else {
                    let definition = self.info.rtt(heap_type as usize);
                    if definition.structural.kind() == RttKind::Function {
                        vfail_if!(
                            self,
                            !self.info.is_subtype(reference, ref_type(true, TypeKind::Funcref)),
                            "{} to type {} expected a funcref",
                            op_name,
                            self.ty(reference)
                        );
                    } else {
                        vfail_if!(
                            self,
                            !self.info.is_subtype(reference, ref_type(true, TypeKind::Anyref)),
                            "{} to type {} expected a subtype of anyref",
                            op_name,
                            self.ty(reference)
                        );
                    }
                    result_type_index = self.info.type_index_of(heap_type as usize);
                }
                let allow_null = op == ExtGCOpType::RefCastNull || op == ExtGCOpType::RefTestNull;
                self.hook(Hook::ExtGC(op))?;
                if op == ExtGCOpType::RefCast || op == ExtGCOpType::RefCastNull {
                    self.expression_stack
                        .push(Type::new(if allow_null { TypeKind::RefNull } else { TypeKind::Ref }, result_type_index));
                } else {
                    self.expression_stack.push(i32_type);
                }
            }
            ExtGCOpType::BrOnCast | ExtGCOpType::BrOnCastFail => {
                let op_name = if op == ExtGCOpType::BrOnCast { "br_on_cast" } else { "br_on_cast_fail" };
                let flags = match self.parser.parse_uint8() {
                    Some(flags) => flags,
                    None => return self.vfail(format!("can't get flags byte for {}", op_name)),
                };
                vfail_if!(self, flags & 0xFC != 0, "reserved bits set in flags byte for {}", op_name);
                let has_null1 = flags & 0x1 != 0;
                let has_null2 = flags & 0x2 != 0;
                let target = self.parse_branch_target(0)?;
                let heap_type1 = parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "can't get first heap type for {}",
                    op_name
                );
                let heap_type2 = parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "can't get second heap type for {}",
                    op_name
                );
                let type_index1 = self.heap_type_index_of(heap_type1);
                let type_index2 = self.heap_type_index_of(heap_type2);
                let kind_of = |nullable: bool| if nullable { TypeKind::RefNull } else { TypeKind::Ref };

                // Manually pop the stack in order to avoid decreasing the stack size, as we will immediately put it back.
                pfail_if!(
                    self,
                    self.expression_stack.len() == self.current_stack_begin,
                    "can't pop empty stack in {}",
                    op_name
                );
                let reference = self.expression_stack.pop().unwrap();
                let source_type = Type::new(kind_of(has_null1), type_index1);
                let target_type = Type::new(kind_of(has_null2), type_index2);
                vfail_if!(
                    self,
                    !self.info.is_subtype(reference, source_type),
                    "{} to type {} expected a reference type with source heaptype",
                    op_name,
                    self.ty(reference)
                );
                vfail_if!(
                    self,
                    !self.info.is_subtype(target_type, source_type),
                    "target heaptype was not a subtype of source heaptype for {}",
                    op_name
                );
                // Depending on the op, the ref gets typed with targetType or srcType \ targetType in the branches.
                let (branch_target_type, non_taken_type) = if op == ExtGCOpType::BrOnCast {
                    (target_type, Type::new(kind_of(has_null1 && !has_null2), type_index1))
                } else {
                    (Type::new(kind_of(has_null1 && !has_null2), type_index1), target_type)
                };
                // Put the ref back on the stack to check the branch type.
                self.expression_stack.push(branch_target_type);
                let index = self.control_index(target);
                self.check_branch_target(index, true)?;
                self.expression_stack.pop();
                self.expression_stack.push(non_taken_type);
                self.hook(Hook::ExtGC(op))?;
            }
            ExtGCOpType::AnyConvertExtern => {
                let reference = pop_or_fail!(self, "any.convert_extern");
                vfail_if!(
                    self,
                    !is_ref_to(reference, TypeKind::Externref),
                    "any.convert_extern reference to type {} expected {}",
                    self.ty(reference),
                    TypeKind::Externref.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(ref_type(reference.is_nullable(), TypeKind::Anyref));
            }
            ExtGCOpType::ExternConvertAny => {
                let reference = pop_or_fail!(self, "extern.convert_any");
                vfail_if!(
                    self,
                    !self.info.is_subtype(reference, ref_type(true, TypeKind::Anyref)),
                    "extern.convert_any reference to type {} expected {}",
                    self.ty(reference),
                    TypeKind::Anyref.name()
                );
                self.hook(Hook::ExtGC(op))?;
                self.expression_stack.push(ref_type(reference.is_nullable(), TypeKind::Externref));
            }
        }
        Ok(())
    }

    /// O `case ExtGC` de `parseUnreachableExpression`.
    pub(super) fn parse_unreachable_ext_gc(&mut self) -> PartialResult {
        self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse extended GC opcode");
        let ext = self.current_ext_op;
        let Some(op) = ExtGCOpType::from_value(ext) else {
            return self.pfail(format!("invalid extended GC op {}", ext));
        };
        match op {
            ExtGCOpType::RefI31
            | ExtGCOpType::I31GetS
            | ExtGCOpType::I31GetU
            | ExtGCOpType::ArrayLen
            | ExtGCOpType::AnyConvertExtern
            | ExtGCOpType::ExternConvertAny => {}
            ExtGCOpType::ArrayNew => self.skip_u32("can't get type index immediate for array.new in unreachable context")?,
            ExtGCOpType::ArrayNewDefault => {
                self.skip_u32("can't get type index immediate for array.new_default in unreachable context")?
            }
            ExtGCOpType::ArrayNewFixed => {
                self.skip_u32("can't get type index immediate for array.new_fixed in unreachable context")?;
                self.skip_u32("can't get argument count for array.new_fixed in unreachable context")?;
            }
            ExtGCOpType::ArrayNewData => {
                self.skip_u32("can't get type index immediate for array.new_data in unreachable context")?;
                self.skip_u32("can't get data segment index for array.new_data in unreachable context")?;
            }
            ExtGCOpType::ArrayNewElem => {
                self.skip_u32("can't get type index immediate for array.new_elem in unreachable context")?;
                self.skip_u32("can't get elements segment index for array.new_elem in unreachable context")?;
            }
            ExtGCOpType::ArrayGet => self.skip_u32("can't get type index immediate for array.get in unreachable context")?,
            ExtGCOpType::ArrayGetS => self.skip_u32("can't get type index immediate for array.get_s in unreachable context")?,
            ExtGCOpType::ArrayGetU => self.skip_u32("can't get type index immediate for array.get_u in unreachable context")?,
            ExtGCOpType::ArraySet => self.skip_u32("can't get type index immediate for array.set in unreachable context")?,
            ExtGCOpType::ArrayFill => self.skip_u32("can't get type index immediate for array.fill in unreachable context")?,
            ExtGCOpType::ArrayCopy => {
                self.skip_u32("can't get first type index immediate for array.copy in unreachable context")?;
                self.skip_u32("can't get second type index immediate for array.copy in unreachable context")?;
            }
            ExtGCOpType::ArrayInitElem => {
                self.skip_u32("can't get first type index immediate for array.init_elem in unreachable context")?;
                self.skip_u32("can't get second type index immediate for array.init_elem in unreachable context")?;
            }
            ExtGCOpType::ArrayInitData => {
                self.skip_u32("can't get first type index immediate for array.init_data in unreachable context")?;
                self.skip_u32("can't get second type index immediate for array.init_data in unreachable context")?;
            }
            ExtGCOpType::StructNew => {
                self.parse_struct_type_index("struct.new")?;
            }
            ExtGCOpType::StructNewDefault => {
                self.parse_struct_type_index("struct.new_default")?;
            }
            ExtGCOpType::StructGet => {
                self.parse_struct_type_index_and_field_index("struct.get")?;
            }
            ExtGCOpType::StructGetS => {
                self.parse_struct_type_index_and_field_index("struct.get_s")?;
            }
            ExtGCOpType::StructGetU => {
                self.parse_struct_type_index_and_field_index("struct.get_u")?;
            }
            ExtGCOpType::StructSet => {
                self.parse_struct_type_index_and_field_index("struct.set")?;
            }
            ExtGCOpType::RefTest | ExtGCOpType::RefTestNull | ExtGCOpType::RefCast | ExtGCOpType::RefCastNull => {
                let op_name = if op == ExtGCOpType::RefCast || op == ExtGCOpType::RefCastNull { "ref.cast" } else { "ref.test" };
                parse_or_fail!(self, self.parser.parse_heap_type(self.info), "can't get heap type for {}", op_name);
            }
            ExtGCOpType::BrOnCast | ExtGCOpType::BrOnCastFail => {
                let op_name = if op == ExtGCOpType::BrOnCast { "br_on_cast" } else { "br_on_cast_fail" };
                let flags = match self.parser.parse_uint8() {
                    Some(flags) => flags,
                    None => return self.vfail(format!("can't get flags byte for {}", op_name)),
                };
                vfail_if!(self, flags & 0xFC != 0, "reserved bits set in flags byte for {}", op_name);
                let has_null1 = flags & 0x1 != 0;
                let has_null2 = flags & 0x2 != 0;
                self.parse_branch_target(0)?;
                let heap_type1 = parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "can't get first heap type for {}",
                    op_name
                );
                let heap_type2 = parse_or_fail!(
                    self,
                    self.parser.parse_heap_type(self.info),
                    "can't get second heap type for {}",
                    op_name
                );
                let type_index1 = self.heap_type_index_of(heap_type1);
                let type_index2 = self.heap_type_index_of(heap_type2);
                let kind_of = |nullable: bool| if nullable { TypeKind::RefNull } else { TypeKind::Ref };
                vfail_if!(
                    self,
                    !self.info.is_subtype(Type::new(kind_of(has_null2), type_index2), Type::new(kind_of(has_null1), type_index1)),
                    "target heaptype was not a subtype of source heaptype for {}",
                    op_name
                );
            }
        }
        Ok(())
    }

    /// O `case ExtAtomic` de `parseExpression`.
    pub(super) fn parse_ext_atomic(&mut self) -> PartialResult {
        self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse atomic extended opcode");
        let ext = self.current_ext_op;
        let Some(op) = ExtAtomicOpType::from_value(ext) else {
            return self.pfail(format!("invalid extended atomic op {}", ext));
        };
        let value = op.value();
        let i32_type = simple_type(TypeKind::I32);
        let i64_type = simple_type(TypeKind::I64);
        match value {
            16..=22 => self.atomic_load(op, simple_type(op.types()[0])),
            23..=29 => self.atomic_store(op, simple_type(op.types()[0])),
            30..=71 => self.atomic_binary_rmw(op, simple_type(op.types()[0])),
            2 => self.atomic_wait(op, i64_type),
            1 => self.atomic_wait(op, i32_type),
            0 => self.atomic_notify(op),
            3 => self.atomic_fence(op),
            72..=78 => {
                let memory_type = if op.name().starts_with("I64") { i64_type } else { i32_type };
                self.atomic_compare_exchange(op, memory_type)
            }
            _ => self.pfail(format!("invalid extended atomic op {}", ext)),
        }
    }

    /// O começo comum dos atômicos com memória: alinhamento, índice e deslocamento.
    fn parse_atomic_memarg(&mut self, op: ExtAtomicOpType, alignment_message: &str) -> Result<u8, String> {
        vfail_if!(self, self.info.memory_count() == 0, "atomic instruction without memory");
        let alignment = parse_or_fail!(self, self.parser.parse_var_uint32(), "{}", alignment_message);
        let (alignment, memory_index) = self.parse_memory_index_and_fixup_alignment(alignment)?;
        let natural = op.log2_alignment();
        pfail_if!(
            self,
            alignment != natural,
            "byte alignment {} does not match against atomic op's natural alignment {}",
            1u64 << alignment,
            1u64 << natural
        );
        self.parse_memory_offset(memory_index)?;
        Ok(memory_index)
    }

    fn check_atomic_pointer(&self, memory_index: u8, pointer: Type, label: String) -> PartialResult {
        let address_kind = self.info.memories[memory_index as usize].address_type.as_wasm_type_kind();
        vfail_if!(self, pointer.kind != address_kind, "{} pointer type mismatch", label);
        Ok(())
    }

    fn atomic_load(&mut self, op: ExtAtomicOpType, memory_type: Type) -> PartialResult {
        let memory_index = self.parse_atomic_memarg(op, "can't get load alignment")?;
        let pointer = pop_or_fail!(self, "load pointer");
        self.check_atomic_pointer(memory_index, pointer, op.value().to_string())?;
        self.hook(Hook::Atomic(op))?;
        self.expression_stack.push(memory_type);
        Ok(())
    }

    fn atomic_store(&mut self, op: ExtAtomicOpType, memory_type: Type) -> PartialResult {
        let memory_index = self.parse_atomic_memarg(op, "can't get store alignment")?;
        let value = pop_or_fail!(self, "store value");
        let pointer = pop_or_fail!(self, "store pointer");
        // O C++ imprime `m_currentOpcode` (o prefixo) aqui.
        let label = self.current_opcode.name().to_string();
        self.check_atomic_pointer(memory_index, pointer, label.clone())?;
        vfail_if!(self, value != memory_type, "{} value type mismatch", label);
        self.hook(Hook::Atomic(op))
    }

    fn atomic_binary_rmw(&mut self, op: ExtAtomicOpType, memory_type: Type) -> PartialResult {
        let memory_index = self.parse_atomic_memarg(op, "can't get load alignment")?;
        let value = pop_or_fail!(self, "value");
        let pointer = pop_or_fail!(self, "pointer");
        self.check_atomic_pointer(memory_index, pointer, op.value().to_string())?;
        vfail_if!(self, value != memory_type, "{} value type mismatch", op.value());
        self.hook(Hook::Atomic(op))?;
        self.expression_stack.push(memory_type);
        Ok(())
    }

    fn atomic_compare_exchange(&mut self, op: ExtAtomicOpType, memory_type: Type) -> PartialResult {
        let memory_index = self.parse_atomic_memarg(op, "can't get load alignment")?;
        let value = pop_or_fail!(self, "value");
        let expected = pop_or_fail!(self, "expected");
        let pointer = pop_or_fail!(self, "pointer");
        self.check_atomic_pointer(memory_index, pointer, op.value().to_string())?;
        vfail_if!(self, expected != memory_type, "{} expected type mismatch", op.value());
        vfail_if!(self, value != memory_type, "{} value type mismatch", op.value());
        self.hook(Hook::Atomic(op))?;
        self.expression_stack.push(memory_type);
        Ok(())
    }

    fn atomic_wait(&mut self, op: ExtAtomicOpType, memory_type: Type) -> PartialResult {
        let memory_index = self.parse_atomic_memarg(op, "can't get load alignment")?;
        let timeout = pop_or_fail!(self, "timeout");
        let value = pop_or_fail!(self, "value");
        let pointer = pop_or_fail!(self, "pointer");
        self.check_atomic_pointer(memory_index, pointer, op.value().to_string())?;
        vfail_if!(self, value != memory_type, "{} value type mismatch", op.value());
        vfail_if!(self, !timeout.is_i64(), "{} timeout type mismatch", op.value());
        self.hook(Hook::Atomic(op))?;
        self.expression_stack.push(simple_type(TypeKind::I32));
        Ok(())
    }

    fn atomic_notify(&mut self, op: ExtAtomicOpType) -> PartialResult {
        let memory_index = self.parse_atomic_memarg(op, "can't get load alignment")?;
        let count = pop_or_fail!(self, "count");
        let pointer = pop_or_fail!(self, "pointer");
        self.check_atomic_pointer(memory_index, pointer, op.value().to_string())?;
        // The spec's definition is saying i64, but all implementations (including tests) are using i32.
        vfail_if!(self, !count.is_i32(), "{} count type mismatch", op.value());
        self.hook(Hook::Atomic(op))?;
        self.expression_stack.push(simple_type(TypeKind::I32));
        Ok(())
    }

    fn atomic_fence(&mut self, op: ExtAtomicOpType) -> PartialResult {
        let flags = parse_or_fail!(self, self.parser.parse_uint8(), "can't get flags");
        pfail_if!(self, flags != 0x0, "flags should be 0x0 but got {}", flags);
        self.hook(Hook::Atomic(op))
    }

    /// O `case ExtAtomic` de `parseUnreachableExpression`.
    pub(super) fn parse_unreachable_ext_atomic(&mut self) -> PartialResult {
        self.current_ext_op = parse_or_fail!(self, self.parser.parse_var_uint32(), "can't parse atomic extended opcode");
        let ext = self.current_ext_op;
        let Some(op) = ExtAtomicOpType::from_value(ext) else {
            return self.pfail(format!("invalid extended atomic op {}", ext));
        };
        if op == ExtAtomicOpType::AtomicFence {
            let flags = parse_or_fail!(self, self.parser.parse_uint8(), "can't get flags");
            pfail_if!(self, flags != 0x0, "flags should be 0x0 but got {}", flags);
            return Ok(());
        }
        self.parse_atomic_memarg(op, "can't get load alignment").map(|_| ())
    }
}
