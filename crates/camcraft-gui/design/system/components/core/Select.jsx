import React from 'react';
export function Select({options=[],size='md',style,...rest}){
  const h={sm:24,md:28,lg:36}[size];
  return <div style={{position:'relative',display:'inline-flex',...style}}>
    <select style={{appearance:'none',WebkitAppearance:'none',height:h,width:'100%',padding:'0 28px 0 8px',background:'var(--surface-3)',color:'var(--text-1)',border:'1px solid var(--border-strong)',borderRadius:'var(--radius-sm)',fontFamily:'var(--font-sans)',fontSize:size==='sm'?12:13,outline:0,cursor:'pointer'}} {...rest}>
      {options.map(o=>typeof o==='string'?<option key={o} value={o}>{o}</option>:<option key={o.value} value={o.value}>{o.label}</option>)}
    </select>
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" style={{position:'absolute',right:6,top:'50%',marginTop:-8,pointerEvents:'none',color:'var(--text-3)'}}><path d="m6 9 6 6 6-6"/></svg>
  </div>;
}