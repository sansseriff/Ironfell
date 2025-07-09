import type { ComponentId, ComponentValue } from './types/component';
import type { EntityId } from './types/entity';
import type { ComponentsEvent, EntityMutation, StreamEvent, TypeRegistryEvent, EntityMutationChange } from './types/message';
import { bevyTypes } from './types/bevy';
import { COMMON_NAMES, bevyCrates } from './types/bevy';

export class SystemState {
    components = new Map<ComponentId, any>();
    registry = new Map<string, any>();
    componentNameToIdMap = new Map<string, ComponentId>();

    entities: Map<
        EntityId,
        Map<
            ComponentId,
            {
                value: ComponentValue;
                disabled: boolean;
            }
        >
    > = new Map();
    childParentMap: Map<EntityId, EntityId | null> = new Map();
    entityNames: Map<EntityId, string> = new Map();
    
    // Client-side deep comparison cache
    private componentValueCache = new Map<EntityId, Map<ComponentId, string>>();

    constructor() {
        // Initialize state


    }


    private setRegistry(types: TypeRegistryEvent['types']) {
        for (const [key, value] of types) {
            this.registry.set(key, value);
        }
    }

    private setComponents(components: ComponentsEvent['components']) {
        for (const component of components) {
            this.components.set(component.id as ComponentId, component);
            // Update the name to ID mapping
            this.componentNameToIdMap.set(component.name, component.id as ComponentId);
        }
    }

    private updateEntity(entity: EntityId, mutation: EntityMutation) {

        // ahh so the parent pointer is a component, and you need to figure out its ID
        const parentComponentId = this.componentNameToIdMap.get(bevyTypes.PARENT);

        if (mutation.kind === 'remove') {
            this.entities.delete(entity);
            this.childParentMap.delete(entity);
            this.entityNames.delete(entity);
            this.componentValueCache.delete(entity); // Clean up cache
            return;
        }

        if (mutation.kind === 'change') {
            const entityComponents = this.entities.get(entity);
            let shouldUpdateName = false;

            if (entityComponents) {
                for (const [removedCommponentId, isDisabled] of mutation.removes) {
                    if (removedCommponentId === parentComponentId) {
                        this.childParentMap.set(entity, null);
                    }

                    const component = entityComponents.get(removedCommponentId);

                    if (!component) {
                        continue;
                    }

                    if (isDisabled) {
                        entityComponents.set(removedCommponentId, {
                            disabled: true,
                            value: component.value,
                        });
                    } else {
                        entityComponents.delete(removedCommponentId);
                        shouldUpdateName = true;
                    }
                }

                for (const [componentId, isDisabled, value] of mutation.changes) {
                    // Client-side deep comparison for frequently changing components
                    const shouldSkipUpdate = this.isComponentValueUnchanged(entity, componentId, value);
                    if (shouldSkipUpdate) {
                        continue;
                    }

                    shouldUpdateName = !entityComponents.has(componentId);
                    entityComponents.set(componentId, {
                        value: value,
                        disabled: isDisabled,
                    });

                    if (
                        componentId === parentComponentId &&
                        !containsHiddenComponent(mutation, this.componentNameToIdMap)
                    ) {
                        this.childParentMap.set(entity, value as EntityId);
                    }
                }

                // Log component changes with entity and component names
                if (mutation.changes.length > 0) {
                    const entityName = this.entityNames.get(entity) || this.getEntityName(entity) || `Entity ${prettyEntityId(entity)}`;
                    const transformComponentId = this.componentNameToIdMap.get('bevy_transform::components::transform::Transform');

                    const changedComponents = mutation.changes.map(([componentId, isDisabled, newValue]) => {
                        const { short_name, name } = this.getComponentName(componentId);
                        const componentName = short_name || name || `Component ${componentId}`;

                        // Show detailed values for Transform component
                        if (componentId === transformComponentId && componentName === 'Transform') {
                            const oldComponent = entityComponents.get(componentId);
                            const oldValue = oldComponent?.value;
                            return `${componentName} (${JSON.stringify(oldValue)} → ${JSON.stringify(newValue)})`;
                        }

                        return componentName;
                    }).join(', ');
                    console.log(`Entity "${entityName}" changed components: ${changedComponents}`);
                }

                this.entities.set(entity, new Map(entityComponents));
            } else {
                // new entity
                if (mutation.removes.length > 0) {
                    console.error(
                        `Receive removed component for untracked entity ${entity}: ${mutation.removes.join(
                            ', ',
                        )}`,
                    );
                }
                this.entities.set(
                    entity,
                    new Map(
                        mutation.changes.map(([componentId, disabled, value]) => [
                            componentId,
                            { value, disabled },
                        ]),
                    ),
                );
                if (!containsHiddenComponent(mutation, this.componentNameToIdMap)) {
                    const parent = mutation.changes.find(
                        ([componentId]) => componentId === parentComponentId,
                    );

                    if (parent) {
                        this.childParentMap.set(entity, parent[1] ? null : (parent[2] as EntityId));
                    } else {
                        this.childParentMap.set(entity, null);
                    }
                }

                shouldUpdateName = true;
            }

            if (shouldUpdateName) {
                this.updateEntityName(entity);
            }

            return;
        }

        console.warn(`Unknown mutation: ${mutation}`);
    }



    private updateEntityName(id: EntityId) {
        const name = this.getEntityName(id);
        if (name) {
            this.entityNames.set(id, name);
        }
    }



    private updateSchedules(schedules: any[]) {
        return;
    }


    public process_update(update: StreamEvent[]) {
        for (const item of update) {
            if (item.kind === 'type_registry') {
                this.setRegistry(item.types);
            } else if (item.kind === 'component') {
                this.setComponents(item.components);
            } else if (item.kind === 'entity') {
                this.updateEntity(item.entity, item.mutation);
            } else if (item.kind === 'schedules') {
                this.updateSchedules(item.schedules);
            } else {
                console.log('Unknown event kind:', item);
            }
        }
    }

    getComponentName(id: ComponentId) {
        const info = this.components.get(id);

        if (!info) {
            return {
                name: undefined,
                short_name: undefined,
            };
        }

        const registeredInfo = this.registry.get(info.name);

        return {
            name: info.name,
            short_name: registeredInfo?.short_name || info.name,
        };
    }


    private getEntityName(id: EntityId) {
        const components = this.entities.get(id);

        if (!components) {
            console.warn(`Entity ${prettyEntityId(id)} does not exist`);
            return 'Non existent entity (BUG)';
        }

        const nameComponentId = this.componentNameToIdMap.get(bevyTypes.NAME);
        if (nameComponentId !== undefined) {
            const nameComponent = components.get(nameComponentId);
            if (nameComponent?.value) {
                return nameComponent.value as string;
            }
        }

        // Search for common component
        for (const commonName in COMMON_NAMES) {
            const componentId = this.componentNameToIdMap.get(commonName);
            if (componentId === undefined) {
                continue;
            }

            if (components.has(componentId)) {
                try {
                    return typeof COMMON_NAMES[commonName] === 'function'
                        ? COMMON_NAMES[commonName](components.get(componentId)!.value)
                        : COMMON_NAMES[commonName];
                } catch {
                    break;
                }
            }
        }

        // Search for non bevy types first
        for (const componentId of components.keys()) {
            const { short_name, name } = this.getComponentName(componentId);
            let isBevyType = false;
            for (const bevyCrate of bevyCrates) {
                if (name?.startsWith(`${bevyCrate}::`)) {
                    isBevyType = true;
                    break;
                }
            }
            if (short_name && !isBevyType) {
                return short_name;
            }
        }

        // search for first suitable component
        for (const componentId of Array.from(components.keys()).sort()) {
            const { short_name, name } = this.getComponentName(componentId);

            // Skip `Parent` and `Children` as they are not confusing
            if (short_name && name !== bevyTypes.PARENT && name !== bevyTypes.CHILDREN) {
                return short_name;
            }
        }

        return 'Entity';
    }


    private isComponentValueUnchanged(entity: EntityId, componentId: ComponentId, newValue: ComponentValue): boolean {
        // Skip deep comparison for null/undefined values
        if (newValue == null) {
            return false;
        }

        const entityCache = this.componentValueCache.get(entity);
        if (!entityCache) {
            // First time seeing this entity, cache the value
            this.componentValueCache.set(entity, new Map([[componentId, JSON.stringify(newValue)]]));
            return false;
        }

        const oldValueStr = entityCache.get(componentId);
        const newValueStr = JSON.stringify(newValue);
        
        if (oldValueStr === newValueStr) {
            // Values are identical, skip this update
            return true;
        }

        // Values are different, update cache
        entityCache.set(componentId, newValueStr);
        return false;
    }

}


const hiddenEntityNames = [bevyTypes.OBSERVER, bevyTypes.SYSTEM_ID_MARKER];

function containsHiddenComponent(
    mutation: EntityMutationChange,
    nameToIdMap: Map<string, ComponentId>,
) {
    for (const [id] of mutation.changes) {
        for (const name of hiddenEntityNames) {
            if (nameToIdMap.get(name) === id) {
                return true;
            }
        }
    }
    return false;
}


function prettyEntityId(id: EntityId) {
    const bid = BigInt(id);
    const index = Number(bid & 0xffffffffn);
    const generation = Number(bid >> 32n);

    return `${index}v${generation}`;
}