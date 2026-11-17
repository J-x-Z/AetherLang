/* The five all-target initializers are static inline SDK functions. Export
 * callable wrappers for consumers that bind plain C symbols rather than C headers.
 * The SDK's own target list selects the components in its matching library. */
#define LLVMInitializeAllTargetInfos aether_header_target_infos
#define LLVMInitializeAllTargets aether_header_targets
#define LLVMInitializeAllTargetMCs aether_header_target_mcs
#define LLVMInitializeAllAsmPrinters aether_header_asm_printers
#define LLVMInitializeAllAsmParsers aether_header_asm_parsers
#include <llvm-c/Target.h>
#undef LLVMInitializeAllTargetInfos
#undef LLVMInitializeAllTargets
#undef LLVMInitializeAllTargetMCs
#undef LLVMInitializeAllAsmPrinters
#undef LLVMInitializeAllAsmParsers

void LLVMInitializeAllTargetInfos(void) { aether_header_target_infos(); }
void LLVMInitializeAllTargets(void) { aether_header_targets(); }
void LLVMInitializeAllTargetMCs(void) { aether_header_target_mcs(); }
void LLVMInitializeAllAsmPrinters(void) { aether_header_asm_printers(); }
void LLVMInitializeAllAsmParsers(void) { aether_header_asm_parsers(); }
