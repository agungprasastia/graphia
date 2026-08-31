require_relative 'helper'

module Analytics
  class DataProcessor
    attr_reader :name

    def initialize(name)
      @name = name
    end

    def execute
      validate
      Helper.do_work
    end

    def validate
      puts "validating"
    end
  end

  def self.run_pipeline
    processor = DataProcessor.new("test")
    processor.execute
  end
end

def top_level_entry
  Analytics.run_pipeline
end
