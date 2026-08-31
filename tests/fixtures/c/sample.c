#include <stdio.h>
#include "helper.h"

typedef struct Point {
    int x;
    int y;
} Point_t;

typedef int custom_int;

void init_point(Point_t* p, int x, int y) {
    p->x = x;
    p->y = y;
}

int calculate_area(int w, int h) {
    helper_print(w);
    return w * h;
}

int main() {
    Point_t pt;
    init_point(&pt, 10, 20);
    calculate_area(pt.x, pt.y);
    return 0;
}
