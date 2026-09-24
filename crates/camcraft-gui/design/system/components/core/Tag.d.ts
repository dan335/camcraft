/** Removable identifier chip (file names, crate names). Mono by default. */
export interface TagProps { children: React.ReactNode; onRemove?: () => void; mono?: boolean; }
export function Tag(props: TagProps): JSX.Element;