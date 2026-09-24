import React from 'react';
export function Switch({checked,defaultChecked,onChange,label,disabled}){
  const [c,setC]=React.useState(!!defaultChecked);const on=checked??c;
  return <label style={{display:'inline-flex',alignItems:'center',gap:8,cursor:disabled?'default':'pointer',opacity:disabled?.4:1,fontSize:13}} onClick={()=>{if(disabled)return;setC(!on);onChange&&onChange(!on)}}>
    <span style={{width:26,height:14,borderRadius:'var(--radius-sm)',background:on?'var(--accent)':'var(--surface-raised)',border:'1px solid '+(on?'var(--accent)':'var(--border-strong)'),position:'relative',transition:'background var(--dur-base) var(--ease-out)'}}>
      <span style={{position:'absolute',top:1,left:on?13:1,width:10,height:10,background:on?'var(--text-on-accent)':'var(--text-3)',borderRadius:1,transition:'left var(--dur-base) var(--ease-out)'}}/>
    </span>{label}</label>;
}