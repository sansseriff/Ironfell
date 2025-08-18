import type { AdapterBridge } from './adapter_bridge';
import { SystemState } from '../system_state.svelte';

export class InspectorClient {
    private bridge: AdapterBridge | null = null;
    readonly state: SystemState;

    constructor(systemState: SystemState) {
        this.state = systemState;
    }

    init(bridge: AdapterBridge) { this.bridge = bridge; }
    private post(data: any) { this.bridge?.post(data); }

    handleUpdate(update: any) { this.state.process_update(update); }

    updateComponent(e: string, c: number, valueJson: string) { this.post({ ty: 'inspector_update_component', entity_id: e, component_id: c, value_json: valueJson }); }
    toggleComponent(e: string, c: number) { this.post({ ty: 'inspector_toggle_component', entity_id: e, component_id: c }); }
    removeComponent(e: string, c: number) { this.post({ ty: 'inspector_remove_component', entity_id: e, component_id: c }); }
    insertComponent(e: string, c: number, v: string) { this.post({ ty: 'inspector_insert_component', entity_id: e, component_id: c, value_json: v }); }
    despawnEntity(e: string, kind = 'Recursive') { this.post({ ty: 'inspector_despawn_entity', entity_id: e, kind }); }
    toggleVisibility(e: string) { this.post({ ty: 'inspector_toggle_visibility', entity_id: e }); }
    reparentEntity(e: string, parentId?: string) { this.post({ ty: 'inspector_reparent_entity', entity_id: e, parent_id: parentId }); }
    spawnEntity(parentId?: string) { this.post({ ty: 'inspector_spawn_entity', parent_id: parentId }); }
}
