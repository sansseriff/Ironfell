import type { TType, TypeName } from './registry';
import type { ComponentInfo, ComponentId, ComponentValue } from './component';
import type { EntityId } from './entity';
import type { ScheduleInfo } from './schedule';

export type StreamEvent = TypeRegistryEvent | ComponentsEvent | EntityEvent | ScheduleEvent;

export type TypeRegistryEvent = {
    kind: 'type_registry';
    types: Array<[TypeName, TType]>;
};

export type ComponentsEvent = {
    kind: 'component';
    components: Array<ComponentInfo & { id: ComponentId }>;
};

export type EntityEvent = {
    kind: 'entity';
    entity: EntityId;
    mutation: EntityMutation;
};

export type ScheduleEvent = {
    kind: 'schedules';
    schedules: ScheduleInfo[];
};

export type EntityMutation = EntityMutationChange | EntityMutationRemove;

export type EntityMutationChange = {
    kind: 'change';
    changes: Array<[ComponentId, boolean, ComponentValue]>;
    removes: Array<[ComponentId, boolean]>;
};
export type EntityMutationRemove = { kind: 'remove' };