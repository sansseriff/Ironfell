export type TypeRegistry = Map<TypeName, TType>;

export type TType = TStruct | TTupleStruct | TTuple | TArray | TMap | TSet | TEnum | TOpaque;

export type TValuePrimitive = number | string | boolean | null;
export type TValueArray = Array<TValue>;
export type TValueObject = { [key: string]: TValue };
export type TValue = TValuePrimitive | TValueArray | TValueObject;

export type TypeName = string;

export type ShortTypeName = string;

export type TStruct = {
    kind: 'struct';
    fields: Array<{ name: string; type: TypeName }>;
    default: TValueObject;
    short_name: ShortTypeName;
};

export type TTupleStruct = {
    kind: 'tuple_struct';
    fields: Array<TypeName>;
    default: TValue; // struct can be new type so it can be any value
    short_name: ShortTypeName;
};

export type TTuple = {
    kind: 'tuple';
    fields: Array<TypeName>;
    short_name: ShortTypeName;
    default: TValueArray; // should be none
};

export type TArray = {
    kind: 'array';
    item: TypeName;
    capacity: number | null; // null is `Vec<T>`
    default: TValueArray;
    short_name: ShortTypeName;
};

export type TMap = {
    kind: 'map';
    key: TypeName;
    value: TypeName;
    default: TValueObject;
    short_name: ShortTypeName;
};

export type TSet = {
    kind: 'set';
    item: TypeName;
    default: TValueArray;
    short_name: ShortTypeName;
};

export type TEnum = {
    kind: 'enum';
    name: string;
    variants: Array<TEnumVariant>;
    default: TValue;
    short_name: ShortTypeName;
};

export type TEnumVariant = TEnumVariantStruct | TEnumVariantTuple | TEnumVariantUnit;

export type TEnumVariantStruct = {
    name: string;
    kind: 'struct';
    fields: Array<{ name: string; type: TypeName }>;
};

export type TEnumVariantTuple = {
    name: string;
    kind: 'tuple';
    fields: Array<TypeName>;
};

export type TEnumVariantUnit = {
    kind: 'unit';
    name: string;
};

export type TOpaque = {
    kind: 'opaque';
    default: TValuePrimitive; // TODO maybe narrow type to string | number | null?
    short_name: ShortTypeName;
};
