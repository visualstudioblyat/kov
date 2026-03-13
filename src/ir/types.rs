// IR-level types — flattened from the AST type system.
// At IR level, peripheral ownership / register types are erased
// to their underlying primitive. Safety was checked during type checking.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrType {
    I8,
    I16,
    I32,   // default for rv32
    I64,
    Bool,  // i1, lowered to i32 in codegen
    Ptr,   // pointer-width integer (i32 on rv32)
    Void,
}

impl IrType {
    pub fn size_bytes(&self) -> u32 {
        match self {
            IrType::I8 | IrType::Bool => 1,
            IrType::I16 => 2,
            IrType::I32 | IrType::Ptr => 4,
            IrType::I64 => 8,
            IrType::Void => 0,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64)
    }
}
