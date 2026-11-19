// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface ISequenceDiagram extends com.change_vision.jude.api.inf.model.IDiagram {
    public abstract com.change_vision.jude.api.inf.model.IInteraction getInteraction();
    public abstract com.change_vision.jude.api.inf.presentation.IPresentation[] getPresentations() throws com.change_vision.jude.api.inf.exception.InvalidUsingException;
    public abstract void setProperty(java.lang.String p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
