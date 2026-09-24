/** Text/number input. mono+unit for machine parameters (mm, mm/min). */
export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> { mono?: boolean; unit?: string; prefix?: string; size?: 'sm'|'md'|'lg'; invalid?: boolean; }
export function Input(props: InputProps): JSX.Element;