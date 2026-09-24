import React from 'react';
export function Radio({options=[],value,defaultValue,onChange,name,direction='column'}){
  const [v,setV]=React.useState(defaultValue);const cur=value??v;
  return <div role="radiogroup" style={{display:'flex',flexDirection:direction,gap:direction==='row'?16:8}}>
    {options.map(o=>{const {value:val,label}=typeof o==='string'?{value:o,label:o}:o;const on=cur===val;
      return <label key={val} style={{display:'inline-flex',alignItems:'center',gap:8,cursor:'pointer',fontSize:13}} onClick={()=>{setV(val);onChange&&onChange(val)}}>
        <span style={{width:14,height:14,borderRadius:'50%',border:'1px solid '+(on?'var(--accent)':'var(--border-strong)'),background:'var(--surface-1)',display:'inline-flex',alignItems:'center',justifyContent:'center'}}>{on&&<span style={{width:6,height:6,borderRadius:'50%',background:'var(--accent)'}}/>}</span>{label}</label>})}
  </div>;
}