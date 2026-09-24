import React from 'react';
export function Tabs({tabs=[],value,defaultValue,onChange,size='md'}){
  const [v,setV]=React.useState(defaultValue??(tabs[0]&&(tabs[0].value??tabs[0])));const cur=value??v;
  return <div role="tablist" style={{display:'flex',gap:0,borderBottom:'1px solid var(--border)'}}>
    {tabs.map(t=>{const {value:val,label,count}=typeof t==='string'?{value:t,label:t}:t;const on=cur===val;
      return <button key={val} role="tab" aria-selected={on} onClick={()=>{setV(val);onChange&&onChange(val)}} style={{height:size==='sm'?28:34,padding:'0 12px',background:'none',border:0,borderBottom:'2px solid '+(on?'var(--accent)':'transparent'),marginBottom:-1,color:on?'var(--text-1)':'var(--text-3)',fontFamily:'var(--font-sans)',fontSize:size==='sm'?12:13,fontWeight:500,cursor:'pointer',display:'inline-flex',alignItems:'center',gap:6}}>{label}{count!=null&&<span style={{fontFamily:'var(--font-mono)',fontSize:11,color:'var(--text-4)'}}>{count}</span>}</button>})}
  </div>;
}