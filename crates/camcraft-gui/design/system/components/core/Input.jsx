import React from 'react';
export function Input({mono,unit,prefix,size='md',invalid,style,...rest}){
  const [f,setF]=React.useState(false);const h={sm:24,md:28,lg:36}[size];
  return <div style={{display:'inline-flex',alignItems:'center',height:h,background:'var(--surface-1)',border:'1px solid '+(invalid?'var(--err)':f?'var(--border-focus)':'var(--border-strong)'),borderRadius:'var(--radius-sm)',boxShadow:f?'var(--focus-ring)':'none',transition:'box-shadow var(--dur-fast)',...style}}>
    {prefix&&<span style={{padding:'0 0 0 8px',fontSize:12,color:'var(--text-3)',fontFamily:'var(--font-mono)'}}>{prefix}</span>}
    <input onFocus={()=>setF(true)} onBlur={()=>setF(false)} style={{flex:1,minWidth:0,height:'100%',background:'transparent',border:0,outline:0,padding:'0 8px',color:'var(--text-1)',fontFamily:mono?'var(--font-mono)':'var(--font-sans)',fontSize:size==='sm'?12:13,fontFeatureSettings:'var(--font-numeric)',textAlign:mono&&unit?'right':'left'}} {...rest}/>
    {unit&&<span style={{padding:'0 8px 0 0',fontSize:12,color:'var(--text-3)',fontFamily:'var(--font-mono)'}}>{unit}</span>}
  </div>;
}