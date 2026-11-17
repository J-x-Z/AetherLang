// A bounded wasm host allocator for exercising the real target ABI. It never
// emulates Vec: all indexing, growth, limits, ownership and copying are Aether.
const fs = require('fs');
let instance, next = 131072, calls = 0, fail = false;
const live = new Map();
function number(value) {
  if (typeof value !== 'number') throw Error('size_t/pointer must cross wasm ABI as i32, not i64');
  return value >>> 0;
}
function allocate(size) {
  size = number(size);
  if (size > 65536 || next + size + 32 > instance.exports.memory.buffer.byteLength)
    throw Error('oversized allocation reached allocator: ' + size);
  const pointer = (next + 15) & ~15;
  next = pointer + size + 16;
  live.set(pointer, size);
  return pointer;
}
function allocation(size) {
  ++calls; number(size);
  if (fail) { fail = false; return 0; }
  return allocate(size);
}
function release(pointer) {
  pointer = number(pointer);
  if (pointer && !live.delete(pointer)) throw Error('invalid/double free ' + pointer);
}
const imports = {env: {
  malloc: allocation,
  realloc(pointer, size) {
    pointer = number(pointer); size = number(size); ++calls;
    if (fail) { fail = false; return 0; }
    if (pointer && !live.has(pointer)) throw Error('invalid realloc');
    const previous = live.get(pointer) || 0;
    const result = allocate(size);
    new Uint8Array(instance.exports.memory.buffer).copyWithin(result, pointer, pointer + Math.min(previous, size));
    release(pointer);
    return result;
  },
  free: release,
  memcpy(destination, source, size) {
    destination = number(destination); source = number(source); size = number(size);
    new Uint8Array(instance.exports.memory.buffer).copyWithin(destination, source, source + size);
    return destination;
  },
  calloc(count, size) {
    count = number(count); size = number(size);
    const pointer = allocation(count * size);
    if (pointer) new Uint8Array(instance.exports.memory.buffer).fill(0, pointer, pointer + count * size);
    return pointer;
  },
  r32_fail() { fail = true; },
  r32_calls() { return calls; },
  r32_live() { return live.size; },
}};
instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[2])), imports);
const result = instance.exports.vec32();
if (result !== 0 || live.size !== 0) throw Error('Vec32 result=' + result + ' live=' + live.size);
