#include <iostream>
#include <vector>
#include "helper.h"

namespace Engine {

class Renderer {
public:
    Renderer() = default;
    void renderScene();
    int getFps() {
        return calculateFps();
    }
private:
    int calculateFps();
};

struct Buffer {
    int size;
};

using BufferList = std::vector<Buffer>;

void Renderer::renderScene() {
    helper_print(60);
}

int Renderer::calculateFps() {
    return 60;
}

void runEngine() {
    Renderer r;
    r.renderScene();
}

} // namespace Engine
