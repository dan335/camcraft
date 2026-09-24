import React from 'react';
export function Checkbox({checked,defaultChecked,onChange,label,disabled}){
  const [c,setC]=React.useState(!!defaultChecked);const on=checked??c;
  return <label style={{display:'inline-flex',alignItems:'center',gap:8,cursor:disabled?'default':'pointer',opacity:disabled?.4:1,fontSize:13,color:'var(--text-1)'}} onClick={()=>{if(disabled)return;setC(!on);onChange&&onChange(!on)}}>
    <span style={{width:14,height:14,border:'1px solid '+(on?'var(--accent)':'var(--border-strong)'),background:on?'var(--accent)':'var(--surface-1)',borderRadius:'var(--radius-sm)',display:'inline-flex',alignItems:'center',justifyContent:'center',transition:'background var(--dur-fast)'}}>{on&&<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="var(--text-on-accent)" strokeWidth="3"><path d="M20 6 9 17l-5-5"/></svg>}</span>{label}
  </label>;
}