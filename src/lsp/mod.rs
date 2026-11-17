//! Stdio LSP using the same resolver, checker, source maps and symbol index as CLI.
use std::{collections::{BTreeMap, HashMap, HashSet}, io::{self, BufRead, Write}, path::{Path,PathBuf}};
use serde_json::{json,Value};
use crate::frontend::{resolve::{ModuleSearch,Resolver}, session::{Diagnostic,Location,Session}, symbols::SymbolIndex};

struct Document { text: String, version: i64 }
struct Server { indexes: BTreeMap<PathBuf,SymbolIndex>, target: String, documents: BTreeMap<PathBuf,Document>, search: ModuleSearch, published: HashSet<String>, shutdown: bool }

pub fn serve(search: ModuleSearch, target: String) -> Result<(),String> {
    crate::types::PointerWidth::for_target(&target)?;
    let stdin=io::stdin();let stdout=io::stdout();
    let mut input=stdin.lock();let mut output=stdout.lock();
    let mut server=Server {indexes:BTreeMap::new(),target,documents:BTreeMap::new(),search,published:HashSet::new(),shutdown:false};
    while let Some(message)=read_message(&mut input)? {
        if message.get("method").and_then(Value::as_str)==Some("exit") { return if server.shutdown {Ok(())}else{Err("LSP exited without shutdown".into())}; }
        for reply in server.handle(message) {write_message(&mut output,&reply)?;}
    }
    Ok(())
}
fn read_message(input:&mut impl BufRead)->Result<Option<Value>,String>{
    let mut size=None;let mut read_any=false;
    loop {
        let mut line=String::new();
        if input.read_line(&mut line).map_err(|e|e.to_string())?==0 {return if read_any {Err("truncated LSP header".into())}else{Ok(None)};}
        read_any=true;
        if line=="\r\n" || line=="\n" {break;}
        if let Some((key,value))=line.split_once(':') {if key.eq_ignore_ascii_case("Content-Length") {size=Some(value.trim().parse::<usize>().map_err(|_|"invalid Content-Length")?);}}
    }
    let size=size.ok_or("missing Content-Length")?;
    if size>16*1024*1024{return Err("LSP message exceeds 16 MiB".into());}
    let mut bytes=vec![0;size];input.read_exact(&mut bytes).map_err(|e|e.to_string())?;
    serde_json::from_slice(&bytes).map(Some).map_err(|e|e.to_string())
}
fn write_message(output:&mut impl Write,value:&Value)->Result<(),String>{
    let body=serde_json::to_vec(value).map_err(|e|e.to_string())?;
    write!(output,"Content-Length: {}\r\n\r\n",body.len()).map_err(|e|e.to_string())?;
    output.write_all(&body).and_then(|_|output.flush()).map_err(|e|e.to_string())
}
fn path_from_uri(uri:&str)->Option<PathBuf>{
    let encoded=uri.strip_prefix("file://")?;
    let encoded=encoded.strip_prefix("localhost/").map(|s|format!("/{s}")).unwrap_or_else(||encoded.into());
    let mut bytes=Vec::new();let input=encoded.as_bytes();let mut i=0;
    while i<input.len(){if input[i]==b'%' {let pair=std::str::from_utf8(input.get(i+1..i+3)?).ok()?;bytes.push(u8::from_str_radix(pair,16).ok()?);i+=3;}else{bytes.push(input[i]);i+=1;}}
    let path=String::from_utf8(bytes).ok()?;
    #[cfg(windows)] let path=path.strip_prefix('/').unwrap_or(&path).to_string();
    Some(Resolver::source_path(Path::new(&path)))
}
fn uri_from_path(path:&str)->String{
    let mut uri=String::from("file://");
    #[cfg(windows)] uri.push('/');
    for byte in path.replace('\\',"/").bytes(){if byte.is_ascii_alphanumeric() || b"/-_.~:".contains(&byte){uri.push(byte as char);}else{uri.push_str(&format!("%{:02X}",byte));}}
    uri
}
fn range(loc:&Location)->Value{json!({"start":{"line":loc.line-1,"character":loc.utf16_column},"end":{"line":loc.end_line-1,"character":loc.end_utf16_column}})}
fn location(loc:&Location)->Value{json!({"uri":uri_from_path(&loc.file),"range":range(loc)})}
fn byte_at(text:&str,pos:&Value)->usize{
    let line=pos["line"].as_u64().unwrap_or(0) as usize;
    let character=pos["character"].as_u64().unwrap_or(0) as usize;
    let start=text.split_inclusive('\n').take(line).map(str::len).sum::<usize>().min(text.len());
    let mut units=0;
    for (i,c) in text[start..].char_indices(){if c=='\n'||units+c.len_utf16()>character{return start+i;}units+=c.len_utf16();if units==character{return start+i+c.len_utf8();}}
    text.len()
}
impl Server {
    fn session(&self,path:&Path)->Result<Session,Diagnostic>{
        let source=match self.documents.get(path){Some(doc)=>doc.text.clone(),None=>std::fs::read_to_string(path).map_err(|e|Diagnostic::message("E9001",e.to_string()))?};
        Session::parse(path,source,self.search.clone(),self.documents.iter().map(|(p,d)|(p.clone(),d.text.clone())).collect::<HashMap<_,_>>())
    }
    fn diagnostics(&mut self)->Vec<Value>{
        self.indexes.clear();
        let mut diagnostics:BTreeMap<String,Vec<Value>>=BTreeMap::new();
        for path in self.documents.keys(){
            let uri=uri_from_path(&path.to_string_lossy());diagnostics.entry(uri.clone()).or_default();
            let result = self.session(path).and_then(|session| {
                let checked = session.check(&self.target);
                self.indexes.insert(path.clone(), SymbolIndex::new(&session, checked.as_ref().ok()));
                checked.map(|_| ())
            });
            if let Err(d)=result {
                let target=d.location.as_ref().map(|l|uri_from_path(&l.file)).unwrap_or(uri);
                let position=d.location.as_ref().map(range).unwrap_or(json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}}));
                let value=json!({"range":position,"severity":1,"code":d.code,"source":"aethc","message":d.message});
                let entries=diagnostics.entry(target).or_default();if !entries.contains(&value){entries.push(value);}
            }
        }
        for uri in &self.published{diagnostics.entry(uri.clone()).or_default();}
        self.published=diagnostics.keys().cloned().collect();
        diagnostics.into_iter().map(|(uri,values)|json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":values}})).collect()
    }
    fn handle(&mut self,message:Value)->Vec<Value>{
        let method=message["method"].as_str().unwrap_or("");let params=&message["params"];let id=message.get("id").cloned();
        let reply=|result:Value|vec![json!({"jsonrpc":"2.0","id":id.clone().unwrap_or(Value::Null),"result":result})];
        let error=|code:i64,text:&str|if let Some(id)=&id{vec![json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":text}})]}else{vec![]};
        if self.shutdown{return error(-32600,"server has shut down");}
        match method {
            "initialize"=>{
                if let Some(path)=params["rootUri"].as_str().and_then(path_from_uri){self.search.paths.push(path);}
                reply(json!({"capabilities":{"positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":1},"hoverProvider":true,"definitionProvider":true,"referencesProvider":true,"documentSymbolProvider":true,"completionProvider":{"triggerCharacters":[":","."]}},"serverInfo":{"name":"aethc","version":env!("CARGO_PKG_VERSION")}}))
            }
            "initialized"|"$/cancelRequest"|"$/setTrace"=>vec![],
            "shutdown"=>{self.shutdown=true;reply(Value::Null)}
            "textDocument/didOpen"=>{
                if let Some(path)=params["textDocument"]["uri"].as_str().and_then(path_from_uri){self.documents.insert(path,Document{text:params["textDocument"]["text"].as_str().unwrap_or("").into(),version:params["textDocument"]["version"].as_i64().unwrap_or(0)});}
                self.diagnostics()
            }
            "textDocument/didChange"=>{
                if let Some(path)=params["textDocument"]["uri"].as_str().and_then(path_from_uri){
                    if let Some(doc)=self.documents.get_mut(&path){let version=params["textDocument"]["version"].as_i64().unwrap_or(doc.version+1);
                        if version>doc.version {if let Some(changes)=params["contentChanges"].as_array(){for change in changes{
                            // Full sync is advertised; accepting ranges also avoids losing edits from clients that send incremental updates.
                            if let Some(text)=change["text"].as_str(){if let Some(r)=change.get("range"){let a=byte_at(&doc.text,&r["start"]);let b=byte_at(&doc.text,&r["end"]);if a<=b{doc.text.replace_range(a..b,text);}}else{doc.text=text.into();}}
                        }}doc.version=version;}
                    }
                }self.diagnostics()
            }
            "textDocument/didClose"=>{if let Some(path)=params["textDocument"]["uri"].as_str().and_then(path_from_uri){self.documents.remove(&path);}self.diagnostics()}
            "textDocument/didSave"=>self.diagnostics(),
            "textDocument/hover"|"textDocument/definition"|"textDocument/references"|"textDocument/documentSymbol"|"textDocument/completion"=>{
                let Some(path)=params["textDocument"]["uri"].as_str().and_then(path_from_uri)else{return error(-32602,"expected file URI");};
                let disk_index;
                let index = if let Some(open) = self.indexes.get(&path) { open }
                    else if let Ok(session) = self.session(&path) {
                        disk_index = SymbolIndex::new(&session, session.check(&self.target).as_ref().ok());
                        &disk_index
                    } else { return reply(Value::Null); };
                let file=path.to_string_lossy();
                if method=="textDocument/documentSymbol"{
                    return reply(Value::Array(index.symbols.iter().filter(|s|s.definition.file==file).map(|s|json!({"name":s.name,"detail":s.type_name,"kind":match s.kind.as_str(){"function"|"method"|"extern function"=>12,"struct"|"union"=>23,"enum"=>10,"constant"=>14,_=>13},"range":range(&s.definition),"selectionRange":range(&s.definition)})).collect()));
                }
                if method=="textDocument/completion"{
                    return reply(json!({"isIncomplete":false,"items":index.symbols.iter().map(|s|json!({"label":s.name,"detail":s.type_name,"kind":if s.kind.contains("function"){3}else{6}})).collect::<Vec<_>>()}));
                }
                let disk_text;
                let text=if let Some(doc)=self.documents.get(&path) { &doc.text } else { disk_text=std::fs::read_to_string(&path).unwrap_or_default(); &disk_text };
                let byte=byte_at(text,&params["position"]);
                let Some(symbol)=index.at(&file,byte)else{return reply(Value::Null);};
                match method{
                    "textDocument/hover"=>reply(json!({"contents":{"kind":"plaintext","value":format!("{}: {}",symbol.name,symbol.type_name)}})),
                    "textDocument/definition"=>reply(location(&symbol.definition)),
                    _ => {
                        let definition = &symbol.definition;
                        let mut hits = BTreeMap::new();
                        for candidate in self.indexes.values().chain(std::iter::once(index)) {
                            for reference in &candidate.references {
                                let target = &candidate.symbols[reference.symbol];
                                if target.definition.file == definition.file && target.definition.start == definition.start &&
                                    target.definition.end == definition.end && target.kind == symbol.kind {
                                    let loc = &reference.location;
                                    hits.insert((&loc.file, loc.start, loc.end), location(loc));
                                }
                            }
                        }
                        if params["context"]["includeDeclaration"].as_bool().unwrap_or(false) {
                            hits.insert((&definition.file, definition.start, definition.end), location(definition));
                        }
                        reply(Value::Array(hits.into_values().collect()))
                    }
                }
            }
            _=>error(-32601,"method not supported"),
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn utf16_position_and_file_uri_round_trip(){let text="a😀中x";assert_eq!(byte_at(text,&json!({"line":0,"character":3})),5);let path="/tmp/你好 space.aeth";assert_eq!(path_from_uri(&uri_from_path(path)).unwrap(),PathBuf::from(path));}
    #[test]fn frames_use_byte_length(){let value=json!({"text":"你好"});let mut bytes=vec![];write_message(&mut bytes,&value).unwrap();assert_eq!(read_message(&mut io::Cursor::new(bytes)).unwrap().unwrap(),value);}
}
