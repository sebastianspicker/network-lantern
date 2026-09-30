export const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

// Form controls share value/disabled/checked access; callers read only what the element provides.
export const control = (id: string) => $<HTMLInputElement>(id);
