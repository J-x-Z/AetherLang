/* Test-only platform glue: thread creation/join. All work and synchronization
   live in the AetherLang workers, including iteration and publication checks. */
#include <pthread.h>
extern void memory_add_worker(void);
extern void memory_publish_worker(void);
extern void memory_consume_worker(void);
static void *add(void *ignored) { (void)ignored; memory_add_worker(); return 0; }
static void *publish(void *ignored) { (void)ignored; memory_publish_worker(); return 0; }
static void *consume(void *ignored) { (void)ignored; memory_consume_worker(); return 0; }
int run_memory_threads(void) {
    pthread_t adders[4], publisher, consumer;
    int created = 0, status = 0;
    for (; created < 4; ++created) {
        if (pthread_create(&adders[created], 0, add, 0) != 0) { status = 1; break; }
    }
    for (int i = 0; i < created; ++i) if (pthread_join(adders[i], 0) != 0) status = 2;
    if (status) return status;
    if (pthread_create(&publisher, 0, publish, 0) != 0) return 3;
    if (pthread_create(&consumer, 0, consume, 0) != 0) {
        pthread_join(publisher, 0);
        return 4;
    }
    if (pthread_join(publisher, 0) != 0) status = 5;
    if (pthread_join(consumer, 0) != 0) status = 6;
    return status;
}
