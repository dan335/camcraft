/** Radio group; the only circles in the system. */
export interface RadioProps { options: Array<string|{value:string;label:React.ReactNode}>; value?: string; defaultValue?: string; onChange?: (v: string) => void; name?: string; direction?: 'row'|'column'; }
export function Radio(props: RadioProps): JSX.Element;