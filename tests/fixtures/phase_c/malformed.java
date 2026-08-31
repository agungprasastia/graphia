package com.example.service;

public class BrokenClass {
    public void brokenMethod( {
        // syntax error
    }

    public void validMethodAfter() {
        System.out.println("recovered");
    }
}
