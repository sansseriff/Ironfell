export type ScheduleInfo = {
    name: string;
    systems: Array<SystemInfo>;
    sets: Array<SetInfo>;
    hierarchies: Array<[string, string[], string[]]>;
    dependencies: Array<[string, string]>;
};

export type SystemInfo = {
    id: string;
    name: string;
};

export type SetInfo = {
    id: string;
    name: string;
};