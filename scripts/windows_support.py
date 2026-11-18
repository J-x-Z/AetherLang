"""Read actual PE headers/imports; does not execute the image."""
import hashlib,struct

def inspect_pe(path, required=()):
 data=path.read_bytes();assert data[:2]==b'MZ';offset=struct.unpack_from('<I',data,60)[0];assert data[offset:offset+4]==b'PE\0\0';machine,sections,_,_,_,opt,flags=struct.unpack_from('<HHIIIHH',data,offset+4);assert machine==0x8664 and flags&2 and not flags&0x2000;assert struct.unpack_from('<H',data,offset+24)[0]==0x20b
 table=offset+24+opt
 def rva(value):
  for i in range(sections):
   at=table+i*40;size,virtual,rawsize,raw=struct.unpack_from('<IIII',data,at+8)
   if virtual<=value<virtual+max(size,rawsize):return raw+value-virtual
  raise AssertionError(('unmapped RVA',value))
 def text(at):return data[at:data.index(b'\0',at)].decode()
 imports=[];directory=struct.unpack_from('<I',data,offset+24+120)[0];at=rva(directory)
 while any(data[at:at+20]):
  lookup,_,_,name,first=struct.unpack_from('<IIIII',data,at);dll=text(rva(name));entry=rva(lookup or first);symbols=[]
  while True:
   value=struct.unpack_from('<Q',data,entry)[0];entry+=8
   if not value:break
   symbols.append('#'+str(value&0xffff) if value>>63 else text(rva(value)+2))
  imports.append(dict(dll=dll,symbols=symbols));at+=20
 symbols={s for item in imports for s in item['symbols']}
 assert not symbols&{'fork','execvp','waitpid','realpath','uname','__error','__errno_location'},symbols
 assert set(required)<=symbols,symbols
 return dict(file=str(path),size=len(data),sha256=hashlib.sha256(data).hexdigest(),machine='AMD64',format='PE32+',imports=imports)
