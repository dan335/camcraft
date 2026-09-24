/** Parameter list: label left, mono value + dim unit right. */
export interface KeyValueProps { items: Array<[React.ReactNode, React.ReactNode, string?]>; columns?: number; }
export function KeyValue(props: KeyValueProps): JSX.Element;