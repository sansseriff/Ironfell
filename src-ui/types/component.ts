import type { TypeName, TValue } from './registry';


export type ComponentName = TypeName;
export type ComponentValue = TValue;
export type ComponentId = number;

export type ComponentInfo = {
    name: ComponentName;
    reflected: boolean;
    required_components: ComponentId[];
};