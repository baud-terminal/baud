#import <Foundation/Foundation.h>
#include <unistd.h>
int main(void) {
    @autoreleasepool {
        NSString *heap = [NSString stringWithFormat:@"heap-%d", getpid()];
        NSLog(@"NSLOGPROBE-MARKER %@", heap);
    }
    return 0;
}
