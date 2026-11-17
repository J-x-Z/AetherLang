/* Check the generated C declarations against C's actual opaque-pointer ABI.
 * A typed __aeth_unit* prototype is incompatible with these declarations. */
void *n05_void_identity(void *);
void *n05_unit_identity(void *);
void **n05_void_slot(void **);
void **n05_unit_slot(void **);
