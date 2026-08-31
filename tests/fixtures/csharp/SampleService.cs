namespace SampleApp;

using System;
using System.Collections.Generic;

public interface IProcessor {
    void Execute();
}

public class DataService : IProcessor {
    public string Name { get; set; }

    public DataService(string name) {
        Name = name;
    }

    public void Execute() {
        RunInternal();
    }

    private void RunInternal() {
        var helper = new CSharpHelper();
        helper.Process();
    }
}

public struct PointStruct {
    public int X;
    public int Y;
}

public record UserDto(string Id, string Email);

public enum Priority {
    Low,
    High
}
