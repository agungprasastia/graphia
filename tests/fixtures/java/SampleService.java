package com.example.service;

import java.util.List;
import com.example.service.Helper;

public class SampleService implements IService {
    private String name;

    public SampleService(String name) {
        this.name = name;
    }

    public void start() {
        processRequest();
    }

    public void processRequest() {
        Helper helper = new Helper();
        helper.doWork();
    }
}

interface IService {
    void start();
}

record UserRecord(String id, String email) {}

enum Status {
    ACTIVE,
    INACTIVE
}
