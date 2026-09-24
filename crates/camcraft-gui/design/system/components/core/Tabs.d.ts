/** Underline tabs with amber indicator and optional mono counts. */
export interface TabsProps { tabs: Array<string|{value:string;label:React.ReactNode;count?:number}>; value?: string; defaultValue?: string; onChange?: (v:string)=>void; size?: 'sm'|'md'; }
export function Tabs(props: TabsProps): JSX.Element;